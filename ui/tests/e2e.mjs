// The page, driven in a real Chrome against a real `keymule ui` with demo
// data: every view in light and dark, the row menus, notes, the add and move
// forms, the usage chart, and (in page mode) pairing, a question, a secret
// typed in the page and a value revealed in it.
//
//   cargo build && cd ui && npm run e2e
//   npm run e2e -- ../target/debug/keymule /tmp/keymule-shots   # also saves screenshots
//
// Needs Chrome (or Chromium; set CHROME to its program) and a debug build
// (it uses KEYMULE_TEST_VAULT). Not in CI yet.
import { chromium } from 'playwright-core'
import { spawn, execFileSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..')
const BIN = path.resolve(process.argv[2] || path.join(REPO, 'target/debug/keymule'))
const OUT = process.argv[3] || null
if (OUT) fs.mkdirSync(OUT, { recursive: true })
const CHROME = process.env.CHROME || [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium', '/usr/bin/chromium-browser',
].find(p => fs.existsSync(p))
if (!CHROME) { console.error('No Chrome found; set CHROME to its program'); process.exit(2) }
const results = []
const children = []
const killAll = () => { for (const c of children) try { c.kill() } catch {} }
process.on('exit', killAll)
const check = (label, ok, detail = '') => { results.push([label, ok]); console.log(`${ok ? 'PASS' : 'FAIL'} ${label}${ok ? '' : `\n     ${detail}`}`) }
const sleep = ms => new Promise(r => setTimeout(r, ms))

function demoHome() {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'kmdemo-'))
  execFileSync('python3', ['-I', path.join(REPO, 'ui/scripts/demo.py'), home])
  return home
}

function startUi(home, extra = {}) {
  const env = { ...process.env, KEYMULE_HOME: home, KEYMULE_TEST_VAULT: path.join(home, 'vault.json'), KEYMULE_NO_UPDATE_CHECK: '1', ...extra }
  const p = spawn(BIN, ['ui', '--no-open', '--port', '0'], { env, stdio: ['ignore', 'pipe', 'inherit'] })
  children.push(p)
  return new Promise((resolve, reject) => {
    let buf = ''
    p.stdout.on('data', d => {
      buf += d
      const m = buf.match(/(http:\/\/127\.0\.0\.1:\d+\/#t=[0-9a-f]{32})/)
      const code = buf.match(/enter: (\d{3}-\d{3})/)
      if (m && (!extra.KEYMULE_APPROVALS || code)) resolve({ p, url: m[1], code: code?.[1], env })
    })
    setTimeout(() => reject(new Error(`ui didn't start: ${buf}`)), 10000)
  })
}

const VIEWS = ['overview', 'credentials', 'servers', 'leaks', 'usage', 'activity']

async function openView(ctx, url, view) {
  const page = await ctx.newPage()
  await page.addInitScript(v => { try { sessionStorage.setItem('keymule-view', v) } catch {} }, view)
  await page.goto(url)
  await page.waitForSelector('#nav button, nav button', { timeout: 10000 })
  await sleep(1200)
  return page
}

const shot = (page, name) => (OUT ? page.screenshot({ path: path.join(OUT, `${name}.png`), fullPage: true }) : null)

const browser = await chromium.launch({ executablePath: CHROME, headless: true })
try {
  // ---- popup mode: every view, light and dark ----
  const home = demoHome()
  const ui = await startUi(home)
  for (const scheme of ['light', 'dark']) {
    const ctx = await browser.newContext({ colorScheme: scheme, viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1 })
    for (const v of VIEWS) {
      const page = await openView(ctx, ui.url, v)
      await shot(page, `${scheme}-${v}`)
      await page.close()
    }
    await ctx.close()
  }

  // ---- interactions (light) ----
  const ctx = await browser.newContext({ colorScheme: 'light', viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1 })
  let page = await openView(ctx, ui.url, 'credentials')
  const errors = []
  page.on('pageerror', e => errors.push(e.message))
  page.on('console', m => { if (m.type() === 'error') errors.push(m.text()) })
  await page.locator('.row', { hasText: 'vps-sydney' }).first().click()
  await sleep(300)
  check('a row opens with its details', await page.locator('.row.open', { hasText: 'vps-sydney' }).count() === 1)
  check('opening a row filters activity to it', await page.locator('.chip', { hasText: 'vps-sydney' }).count() === 1)
  await page.locator('.row', { hasText: 'vps-sydney' }).first().locator('.menuwrap button').first().click()
  await sleep(200)
  check('the ⋯ menu opens', await page.locator('.menu .mi').count() >= 3)
  await shot(page, 'i-row-open-menu')
  await page.keyboard.press('Escape')
  await sleep(150)
  check('Escape closes the menu', await page.locator('.menu').count() === 0)
  await page.locator('.row.open .notebtn').click()
  await sleep(200)
  const note = page.locator('textarea.notefield')
  check('the note editor opens focused', await note.evaluate(el => el === document.activeElement))
  await note.fill('Production VPS in Sydney')
  await shot(page, 'i-note-edit')
  await note.press('Enter')
  await sleep(1500)
  const toml = fs.readFileSync(path.join(home, 'credentials.toml'), 'utf8')
  check('Enter saves the note', toml.includes('Production VPS in Sydney'), toml.slice(0, 200))
  await page.selectOption('#svc', 'ssh')
  await sleep(150)
  check('choosing SSH shows the SSH fields', await page.locator('#sshhost').isVisible() && !(await page.locator('#url').isVisible()))
  await page.selectOption('#sshauth', 'keyfile')
  await sleep(150)
  check('key file shows its path field', await page.locator('#sshkeyfile').isVisible())
  await shot(page, 'i-add-ssh')
  await page.close()

  page = await openView(ctx, ui.url, 'servers')
  await page.locator('.row', { hasText: 'old-db' }).first().locator('.menuwrap button').first().click()
  await page.locator('.menu .mi', { hasText: 'Move to Credentials' }).click()
  await sleep(200)
  check('Move to Credentials shows the form', await page.locator('form.convert').count() === 1)
  await shot(page, 'i-convert')
  await page.close()

  page = await openView(ctx, ui.url, 'usage')
  const bars = page.locator('.uhit')
  const n = await bars.count()
  check('the usage chart has a bar per day', n === 30, `bars: ${n}`)
  await bars.nth(n - 2).hover()
  await sleep(200)
  check('hovering a bar shows its day', await page.locator('.utip').isVisible())
  await shot(page, 'i-usage-tip')
  await page.locator('button', { hasText: 'Show as a table' }).click()
  await sleep(200)
  check('the table shows', await page.locator('.utable .utr').count() > 5)
  await page.locator('.seg button', { hasText: '7 days' }).click()
  await sleep(800)
  check('7 days redraws the chart', await page.locator('.uhit').count() === 7)
  await page.close()
  page = await openView(ctx, ui.url, 'activity')
  check('activity lists many lines', await page.locator('.act').count() >= 40, String(await page.locator('.act').count()))
  page.on('pageerror', e => errors.push(e.message))
  await page.close()
  check('no page errors', errors.length === 0, errors.join('\n'))
  await ctx.close()
  ui.p.kill()

  // ---- page mode: pairing and questions ----
  const home2 = demoHome()
  const ui2 = await startUi(home2, { KEYMULE_APPROVALS: 'page' })
  const ctx2 = await browser.newContext({ colorScheme: 'light', viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1 })
  page = await openView(ctx2, ui2.url, 'overview')
  await sleep(800)
  check('page mode shows the pairing banner', await page.locator('.pairing', { hasText: 'Answer keymule' }).count() === 1)
  await shot(page, 'p-pairing')

  // Pair this browser from the banner.
  await page.locator('#paircode').fill(ui2.code)
  await page.locator('.pairing button', { hasText: 'Pair this browser' }).click()
  for (let i = 0; i < 50 && await page.locator('#paircode').count(); i++) await sleep(200)
  check('pairing from the banner works', await page.locator('#paircode').count() === 0)

  // A delete question: this (paired) tab gets the real buttons, a second,
  // unpaired browser only Hide / Deny.
  const rm = spawn(BIN, ['rm', 'old-db', '--dialog'], { env: ui2.env, stdio: ['ignore', 'pipe', 'pipe'] })
  children.push(rm)
  let rmOut = ''
  rm.stdout.on('data', d => { rmOut += d }); rm.stderr.on('data', d => { rmOut += d })
  await page.waitForSelector('.ask .askfoot', { timeout: 15000 })
  await sleep(400)
  check('the question shows in the page', await page.locator('.ask h2').count() === 1)
  check('paired: the safe button has focus', await page.evaluate(() => document.activeElement?.dataset?.safe === '1'))
  await shot(page, 'p-ask-paired')
  const other = await browser.newContext({ colorScheme: 'light', viewport: { width: 1280, height: 860 }, deviceScaleFactor: 1 })
  const op = await openView(other, ui2.url, 'overview')
  await op.waitForSelector('.ask .askpair', { timeout: 15000 })
  await sleep(300)
  check('unpaired browser: can only hide or deny', await op.locator('.ask .askfoot .btn').count() === 2 && await op.locator('.ask .askpair').count() === 1)
  await shot(op, 'p-ask-unpaired')
  await other.close()
  await page.locator('.ask .askfoot .btn[data-safe]').click()
  const rmCode = await new Promise(r => rm.on('exit', r))
  check('Cancel answers the question: nothing deleted', rmCode !== 0 && fs.readFileSync(path.join(home2, 'credentials.toml'), 'utf8').includes('[servers.old-db]'), rmOut)

  // A secret typed in the page: rotate a password.
  const before = JSON.parse(fs.readFileSync(path.join(home2, 'vault.json'), 'utf8'))['db-pass']
  const rot = spawn(BIN, ['rotate', 'db-pass', '--dialog', '--no-verify'], { env: ui2.env, stdio: ['ignore', 'pipe', 'pipe'] })
  children.push(rot)
  let rotOut = ''
  rot.stdout.on('data', d => { rotOut += d }); rot.stderr.on('data', d => { rotOut += d })
  await page.waitForSelector('#askval', { timeout: 15000 })
  await sleep(300)
  check('the secret field has focus', await page.evaluate(() => document.activeElement?.id === 'askval'))
  await page.locator('#askval').fill('rotated-in-the-page-42')
  check('the secret is hidden', await page.locator('#askval').getAttribute('type') === 'password')
  await page.locator('.peek').click()
  check('the eye shows it', await page.locator('#askval').getAttribute('type') === 'text')
  check('the cursor stays in the field', await page.evaluate(() => document.activeElement?.id === 'askval'))
  await shot(page, 'p-ask-secret')
  await page.locator('#askval').press('Enter')
  const rotCode = await new Promise(r => rot.on('exit', r))
  const after = JSON.parse(fs.readFileSync(path.join(home2, 'vault.json'), 'utf8'))['db-pass']
  check('Enter saves: the vault has the new value', rotCode === 0 && after !== before && after.includes('rotated-in-the-page-42'), rotOut)
  await sleep(600)
  check('the card closes after answering', await page.locator('.ask').count() === 0 || !(await page.locator('#ask').isVisible()))

  // Reveal, answered in the page: the value opens in this tab.
  await page.close()
  page = await openView(ctx2, ui2.url, 'credentials')
  await page.locator('.row', { hasText: 'linear' }).first().locator('button', { hasText: 'Reveal' }).click()
  await page.waitForSelector('.ask .askfoot', { timeout: 15000 })
  await sleep(300)
  await shot(page, 'p-reveal-ask')
  await page.locator('.ask .askfoot .btn.primary').click()
  await page.waitForSelector('#reveal .revealval', { timeout: 5000 })
  const shown = await page.locator('#reveal .revealval').first().textContent()
  check('Reveal opens the value in this tab', shown.includes('demo-secret-for-linear'), shown)
  await shot(page, 'p-reveal')
  await page.keyboard.press('Escape')
  await sleep(200)
  check('Escape closes the value', !(await page.locator('#reveal .revealval').count()))
  await ctx2.close()
  ui2.p.kill()
} catch (e) {
  check('flow ran to the end', false, e.stack)
} finally {
  await browser.close()
  killAll()
}
const failed = results.filter(r => !r[1]).length
console.log(`${results.length - failed}/${results.length} passed`)
process.exit(failed ? 1 : 0)
