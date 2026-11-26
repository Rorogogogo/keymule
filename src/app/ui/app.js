// keymule ui. Everything shown here comes from keymule commands that never
// print secrets; buttons run commands that open keymule's own dialogs.
'use strict'

const $ = id => document.getElementById(id)

// The token is in the URL fragment keymule opened. Keep it for this tab only
// and take it out of the address bar. Without one, a paired browser logs in
// with its pairing key (see login below).
let token = (() => {
  const m = location.hash.match(/t=([0-9a-f]{32})/)
  if (m) {
    try { sessionStorage.setItem('keymule-token', m[1]) } catch {}
    history.replaceState(null, '', location.pathname)
    return m[1]
  }
  try { return sessionStorage.getItem('keymule-token') } catch { return null }
})()

const state = {
  list: null,
  audit: [],
  version: '',
  live: null, // true, false, or null before the first answer
  plan: null, // { text, has, names, summary }
  planOpen: false,
  confirmApply: false,
  doctor: null, // null, 'running', or the report
  more: {}, // extra rows shown per list, 20 at a time
  menu: null, // the row whose ⋯ menu is open
  busy: new Set(),
  filter: null,
  describing: null, // the row whose description is being edited
  converting: null, // the MCP server secret being moved to Credentials
  view: (() => { try { return sessionStorage.getItem('keymule-view') || 'overview' } catch { return 'overview' } })(),
  stats: null, // usage numbers from /api/stats
  urange: (() => { try { return Number(sessionStorage.getItem('keymule-urange')) || 30 } catch { return 30 } })(),
  utable: false, // the usage chart shown as a table
}

// ---- helpers ----------------------------------------------------------------

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag)
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v == null || v === false) continue
    if (k === 'class') el.className = v
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v)
    else el.setAttribute(k, v === true ? '' : v)
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid == null || kid === false) continue
    el.append(kid instanceof Node ? kid : String(kid)) // text, never HTML
  }
  return el
}

// Stroke icons, 24x24, drawn as SVG nodes (no innerHTML).
const ICONS = {
  key: [['circle', { cx: 7.5, cy: 15.5, r: 4.5 }], ['path', { d: 'M10.7 12.3 20 3m-3 3 3 3m-6 0 2 2' }]],
  server: [['rect', { x: 3, y: 3.5, width: 18, height: 7, rx: 2 }], ['rect', { x: 3, y: 13.5, width: 18, height: 7, rx: 2 }], ['path', { d: 'M7 7h.01M7 17h.01' }]],
  warn: [['path', { d: 'M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z' }], ['path', { d: 'M12 9v4M12 17h.01' }]],
  eye: [['path', { d: 'M1.5 12S5.5 5 12 5s10.5 7 10.5 7-4 7-10.5 7S1.5 12 1.5 12z' }], ['circle', { cx: 12, cy: 12, r: 3 }]],
  eyeOff: [['path', { d: 'M10.7 5.1A10.7 10.7 0 0 1 22.5 12a17 17 0 0 1-2 2.9M14.1 14.2a3 3 0 0 1-4.2-4.2M17.5 17.5A10.5 10.5 0 0 1 1.5 12a17 17 0 0 1 5-5.5M2 2l20 20' }]],
  check: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'm8 12.5 2.5 2.5L16 9.5' }]],
  x: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'm15 9-6 6M9 9l6 6' }]],
  shield: [['path', { d: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z' }], ['path', { d: 'm9 12 2 2 4-4' }]],
  spin: [['path', { d: 'M21 12a9 9 0 1 1-6.2-8.56' }]],
  home: [['rect', { x: 3, y: 3, width: 7, height: 9, rx: 1.5 }], ['rect', { x: 14, y: 3, width: 7, height: 5, rx: 1.5 }], ['rect', { x: 14, y: 12, width: 7, height: 9, rx: 1.5 }], ['rect', { x: 3, y: 16, width: 7, height: 5, rx: 1.5 }]],
  pulse: [['path', { d: 'M3 12h4l3-8 4 16 3-8h4' }]],
  sidebar: [['rect', { x: 3, y: 4, width: 18, height: 16, rx: 2.5 }], ['path', { d: 'M9 4v16' }]],
  sun: [['circle', { cx: 12, cy: 12, r: 4 }], ['path', { d: 'M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4' }]],
  moon: [['path', { d: 'M20.5 14.1A8.5 8.5 0 1 1 9.9 3.5a6.6 6.6 0 0 0 10.6 10.6z' }]],
  terminal: [['rect', { x: 2.5, y: 4, width: 19, height: 16, rx: 2 }], ['path', { d: 'm7 9 3 3-3 3M13 15h4' }]],
  monitor: [['rect', { x: 2.5, y: 4, width: 19, height: 13, rx: 2 }], ['path', { d: 'M8 21h8M12 17v4' }]],
  more: [['circle', { cx: 5, cy: 12, r: 1 }], ['circle', { cx: 12, cy: 12, r: 1 }], ['circle', { cx: 19, cy: 12, r: 1 }]],
  chev: [['path', { d: 'm9 6 6 6-6 6' }]],
  pen: [['path', { d: 'M12 20h9' }], ['path', { d: 'M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z' }]],
  chart: [['path', { d: 'M3 3v18h18' }], ['path', { d: 'M8 17v-5M13 17V8M18 17v-9' }]],
}
function icon(name) {
  const NS = 'http://www.w3.org/2000/svg'
  const svg = document.createElementNS(NS, 'svg')
  for (const [k, v] of Object.entries({ viewBox: '0 0 24 24', fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round', 'aria-hidden': 'true' })) svg.setAttribute(k, v)
  for (const [tag, attrs] of ICONS[name] || []) {
    const el = document.createElementNS(NS, tag)
    for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
    svg.append(el)
  }
  return svg
}
const ico = (name, cls) => h('span', { class: `ico ${cls || ''}${name === 'spin' ? ' spinning' : ''}`, 'aria-hidden': 'true' }, icon(name))
const badge = (text, kind) => h('span', { class: `badge ${kind || ''}` }, text)
const join = parts => parts.filter(Boolean).flatMap((p, i) => (i ? [' · ', p] : [p]))
const mono = text => h('span', { class: 'mono' }, text)

async function api(path, body, retried) {
  if (!token) await tryLogin()
  const used = token
  const r = await fetch(path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { 'X-Keymule-Token': used || '', 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (r.status === 403) {
    // `keymule ui` restarted with a new token: log in again, once.
    if (!retried && (token !== used || await tryLogin()) && token !== used) return api(path, body, true)
    throw new Error('forbidden')
  }
  return r.json()
}

// A browser paired for keymule's questions opens the page without the link:
// it signs a one-off challenge with its pairing key (which can't be read out
// of the browser) and gets the token back. Nothing to log in with until the
// browser is paired.
async function login() {
  const d = await idb('readonly', s => s.get('me')).catch(() => null)
  if (!d) return false
  const post = (path, body) => fetch(path, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
  }).then(r => r.json())
  const c = await post('/login/challenge', {})
  if (!c.ok) return false
  const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, d.keyPair.privateKey, enc(`keymule-login-v1\n${c.nonce}`))
  const r = await post('/login', { device: d.id, nonce: c.nonce, sig: b64(sig) })
  if (!r.ok) return false
  token = r.token
  try { sessionStorage.setItem('keymule-token', token) } catch {}
  return true
}

// One login at a time: a good one is reused for 2 s, and after a failed one
// keymule waits 10 s before trying again.
let loginTry = null
function tryLogin() {
  if (!loginTry) {
    loginTry = login().catch(() => false)
    loginTry.then(ok => setTimeout(() => { loginTry = null }, ok ? 2000 : 10000))
  }
  return loginTry
}

const TOAST_ICON = { ok: 'check', bad: 'x', run: 'spin' }
let toastTimer
function toast(text, kind, sticky) {
  const el = $('toast')
  clearTimeout(toastTimer)
  el.replaceChildren(ico(TOAST_ICON[kind] || 'check', kind || 'ok'), h('span', { class: 'body' }, text))
  el.hidden = false
  if (!sticky) toastTimer = setTimeout(() => { el.hidden = true }, 7000)
}

function when(ts) {
  const d = new Date(ts * 1000)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' })
}

// ---- loading ----------------------------------------------------------------

async function refresh() {
  try {
    const s = await api('/api/state')
    state.list = s.list
    state.audit = s.audit || []
    state.version = s.version
    state.update = s.update || null
    state.host = s.host || 'ui'
    state.live = true
    state.error = s.error
  } catch (e) {
    state.live = false
    state.forbidden = e.message === 'forbidden'
  }
  render()
}

async function loadPlan() {
  try {
    const r = await api('/api/plan', {})
    const text = (r.stdout || '').trim()
    const has = /secrets? in plain text:/.test(text)
    const head = text.split('\nNot adopted:')[0]
    const names = has ? [...head.matchAll(/^ {2}(\S+) {3}/gm)].map(m => m[1]) : []
    state.plan = { text, has, names, summary: has ? text.split('\n')[0].replace(/:$/, '') : '' }
  } catch { state.plan = null }
  render()
}

// ---- actions ----------------------------------------------------------------

async function run(key, start, call, after) {
  if (state.busy.has(key)) return
  state.busy.add(key)
  render()
  toast(start, 'run', true)
  try {
    const r = await call()
    const msg = (r.ok ? r.stdout : r.stderr || r.stdout || 'Something went wrong').trim()
    toast(msg || 'Done.', r.ok ? 'ok' : 'bad')
  } catch (e) {
    toast(`keymule ui isn't reachable: ${e.message}`, 'bad')
  } finally {
    state.busy.delete(key)
    await refresh()
    if (after) await after()
  }
}

const rotate = name => run(`rotate:${name}`,
  `A keymule dialog is open for "${name}". Type the new value there.`,
  () => api('/api/rotate', { name }), loadPlan)

$('add').addEventListener('submit', e => {
  e.preventDefault()
  const service = $('svc').value
  if (service === 'ssh') {
    const body = {
      service, host: $('sshhost').value.trim(), username: $('sshuser').value.trim(),
      port: $('sshport').value.trim(), auth: $('sshauth').value, key_file: $('sshkeyfile').value.trim(),
    }
    if (body.auth === 'keyfile' && !body.key_file) { toast('Give the key file\'s path, like ~/.ssh/id_ed25519.', 'bad'); return }
    if (!body.host || !body.username) { toast('Give the server\'s host or IP and the username.', 'bad'); return }
    const msg = body.auth === 'keyfile' ? 'Saving…' : `A keymule dialog is open. Type the ${body.auth === 'key' ? 'private key' : 'password'} there.`
    run('add', msg, () => api('/api/add', body), () => { for (const id of ['sshhost', 'sshuser', 'sshport', 'sshkeyfile']) $(id).value = '' })
    return
  }
  const url = $('url').value.trim()
  run('add', 'A keymule dialog is open. Type the token there.',
    () => api('/api/add', { service, url }), () => { $('url').value = '' })
})

const URL_HINT = {
  github: 'https://api.github.com',
  gitlab: 'https://gitlab.com/api/v4',
  bamboo: 'https://bamboo.yourcompany.com',
  jenkins: 'https://jenkins.yourcompany.com',
  generic: 'https://api.example.com',
}
const setHint = () => {
  const ssh = $('svc').value === 'ssh'
  $('url').placeholder = URL_HINT[$('svc').value] || ''
  $('url').hidden = ssh
  for (const id of ['sshhost', 'sshuser', 'sshport', 'sshauth']) $(id).hidden = !ssh
  $('sshkeyfile').hidden = !ssh || $('sshauth').value !== 'keyfile'
  $('add').querySelector('button').textContent = ssh ? 'Add SSH login' : 'Add credential'
  $('addnote').textContent = ssh
    ? 'You type the password (or paste the key) into a keymule dialog, never into this page. Agents run commands with keymule ssh.'
    : 'You type the token into a keymule dialog, never into this page.'
}
$('svc').addEventListener('change', setHint)
$('sshauth').addEventListener('change', setHint)
setHint()

// The scan runs in keymule's background; the page polls how far it got.
let scanTimer
function applyScan(r) {
  state.scan = r
  if (r.running) state.doctor = 'running'
  else if (r.error) state.doctor = { error: r.error }
  else if (r.report && r.report.entries) {
    if (state.doctor === 'running' || state.doctor === null || state.doctorId !== r.id) state.doctor = r.report
  }
  state.doctorId = r.id
}
async function pollScan() {
  clearTimeout(scanTimer)
  try { applyScan(await api('/api/doctor')) } catch {}
  render()
  if (state.scan?.running) scanTimer = setTimeout(pollScan, 400)
}
$('scan').addEventListener('click', async () => {
  if (state.doctor === 'running') return
  state.doctor = 'running'
  state.scan = null
  state.more.leaks = 0
  render()
  try {
    applyScan(await api('/api/doctor', {}))
  } catch (e) {
    state.doctor = { error: e.message }
  }
  render()
  if (state.scan?.running) scanTimer = setTimeout(pollScan, 400)
})

const PHASES = {
  transcripts: 'Reading agent transcripts', configs: 'Checking agent configs', env: 'Looking for .env files',
  history: 'Checking shell history', stored: 'Comparing with the keys in keymule', done: 'Finishing up',
}
// Transcripts are most of the work; the rest is quick.
const PHASE_AT = { configs: 0.92, env: 0.94, history: 0.96, stored: 0.98, done: 1 }
const size = b => (b >= 1 << 30 ? `${(b / (1 << 30)).toFixed(1)} GB` : `${Math.round(b / (1 << 20))} MB`)
const clock = ms => { const s = Math.floor((ms || 0) / 1000); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` }

// Built once per scan, then updated in place so the spinner and log don't restart.
let scanPanel = null
function scanBox() {
  const fill = h('span', { class: 'fill' })
  const track = h('div', { class: 'scanbar', role: 'progressbar', 'aria-label': 'Scan progress', 'aria-valuemin': '0', 'aria-valuemax': '100' }, fill)
  const phase = h('span', { class: 'phase' }, 'Starting the scan…')
  const time = h('span', { class: 'time' })
  const meta = h('div', { class: 'scanmeta' }, 'Values stay masked, and nothing leaves this Mac.')
  const log = h('div', { class: 'scanlog', 'aria-live': 'polite' })
  const el = h('div', { class: 'scanrun' }, h('div', { class: 'scanhead' }, ico('spin'), phase, time), track, meta, log)
  scanPanel = { el, fill, track, phase, time, meta, log, lines: 0 }
  updateScanBox()
  return el
}
function updateScanBox() {
  const p = scanPanel
  const r = state.scan
  if (!p || !r || !p.el.isConnected) return
  const frac = r.phase === 'transcripts' ? (r.total ? 0.9 * r.done / r.total : 0) : (PHASE_AT[r.phase] ?? 0)
  p.fill.style.transform = `scaleX(${frac})`
  p.track.setAttribute('aria-valuenow', String(Math.round(frac * 100)))
  p.phase.textContent = PHASES[r.phase] || 'Scanning'
  p.time.textContent = clock(r.elapsed_ms)
  const parts = []
  if (r.total) parts.push(`${size(r.done)} of ${size(r.total)}`)
  if (r.phase === 'transcripts' && r.done > 0 && r.elapsed_ms > 2000) {
    const left = (r.total - r.done) / (r.done / r.elapsed_ms) / 1000
    parts.push(left < 5 ? 'almost done' : left < 60 ? `about ${Math.ceil(left / 5) * 5} s left` : `about ${Math.ceil(left / 60)} min left`)
  }
  if (r.lines) parts.push(`${r.found} possible ${r.found === 1 ? 'key' : 'keys'} so far`)
  if (parts.length) p.meta.textContent = parts.join(' · ')
  const fresh = r.lines - p.lines
  if (fresh > 0) {
    for (const line of r.log.slice(-Math.min(fresh, 6))) p.log.append(h('div', { class: 'ln' }, line))
    while (p.log.childElementCount > 6) p.log.firstElementChild.remove()
    p.lines = r.lines
  }
}

function applyPlan() {
  if (!state.confirmApply) {
    state.confirmApply = true
    render()
    setTimeout(() => { state.confirmApply = false; render() }, 4000)
    return
  }
  state.confirmApply = false
  run('adopt', 'Moving the keys into the keychain…', () => api('/api/adopt', {}), loadPlan)
}

function toggleFilter(name) {
  state.filter = state.filter === name ? null : name
  render()
}

// Per-browser conveniences: theme and a collapsed sidebar.
const pref = (k, d) => { try { return localStorage.getItem(`keymule-${k}`) || d } catch { return d } }
const setPref = (k, v) => { try { localStorage.setItem(`keymule-${k}`, v) } catch {} }
const THEMES = { system: ['monitor', 'System theme'], light: ['sun', 'Light theme'], dark: ['moon', 'Dark theme'] }
const NEXT_THEME = { system: 'light', light: 'dark', dark: 'system' }
let theme = pref('theme', 'system')
let collapsed = pref('sidebar', 'open') === 'collapsed'
function applyPrefs() {
  if (theme === 'system') delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = theme
  const [ic, label] = THEMES[theme] || THEMES.system
  $('theme').replaceChildren(icon(ic), h('span', { class: 'label' }, label))
  $('theme').title = `${label} (click to change)`
  document.querySelector('.app').classList.toggle('collapsed', collapsed)
  $('collapse').replaceChildren(icon('sidebar'))
  const what = collapsed ? 'Expand sidebar' : 'Collapse sidebar'
  $('collapse').title = what
  $('collapse').setAttribute('aria-label', what)
}
$('theme').addEventListener('click', () => { theme = NEXT_THEME[theme] || 'system'; setPref('theme', theme); applyPrefs() })
$('collapse').addEventListener('click', () => { collapsed = !collapsed; setPref('sidebar', collapsed ? 'collapsed' : 'open'); applyPrefs(); render() })
applyPrefs()

const VIEWS = [
  ['overview', 'Overview', 'home', 'Your agents use these keys without ever seeing them. Everything stays on this machine.'],
  ['credentials', 'Credentials', 'key', 'API tokens, passwords and SSH logins your agents use through keymule. You type each one into a keymule dialog.'],
  ['servers', 'MCP servers', 'server', 'Secrets keymule hands to your MCP servers when they start, instead of keeping them in config files.'],
  ['leaks', 'Leak check', 'shield', 'Keys that already reached a model, sit in plain text, or are readable by agents.'],
  ['usage', 'Usage', 'chart', 'How often your agents used each key, and how: API calls, commands, MCP servers, and values shown to you.'],
  ['activity', 'Activity', 'pulse', 'Every request and server start that used a key, newest first.'],
]
function go(view) {
  state.view = view
  state.more = {}
  try { sessionStorage.setItem('keymule-view', view) } catch {}
  window.scrollTo(0, 0)
  render()
}
$('allact').addEventListener('click', () => go('activity'))

// ---- drawing ----------------------------------------------------------------

function revealButton(name) {
  const busy = state.busy.has(`reveal:${name}`)
  return h('button', {
    class: 'btn ghost', type: 'button', disabled: busy, title: 'Show the value in a keymule window, never on this page',
    onclick: e => {
      e.stopPropagation()
      run(`reveal:${name}`, ap.pageMode ? `keymule asks here before showing "${name}".` : `Look for the keymule window: it asks before showing "${name}".`,
        () => api('/api/reveal', { name }))
    },
  }, busy ? 'Showing…' : 'Reveal')
}

const remove = name => run(`delete:${name}`, `A keymule window asks before deleting "${name}".`,
  () => api('/api/delete', { name }), loadPlan)

// Open a row and start editing its note.
function editNote(name) {
  state.filter = name
  state.describing = name
  render()
}

// The actions you need less often, behind one button. items: [label, fn, { danger, disabled, title }]
function moreMenu(name, items) {
  const open = state.menu === name
  const list = items.filter(Boolean)
  return h('span', { class: 'menuwrap' },
    h('button', {
      class: 'btn ghost iconly', type: 'button', title: 'More actions',
      'aria-label': `More actions for ${name}`, 'aria-haspopup': 'menu', 'aria-expanded': open ? 'true' : 'false',
      onclick: e => {
        e.stopPropagation()
        state.menu = open ? null : name
        render()
        if (!open) setTimeout(() => document.querySelector('.menu .mi:not(:disabled)')?.focus(), 0)
      },
      onkeydown: e => e.stopPropagation(),
    }, icon('more')),
    open ? h('div', {
      class: 'menu', role: 'menu',
      onclick: e => e.stopPropagation(),
      onkeydown: e => {
        e.stopPropagation()
        const mis = [...e.currentTarget.querySelectorAll('.mi:not(:disabled)')]
        const i = mis.indexOf(document.activeElement)
        if (e.key === 'ArrowDown') { e.preventDefault(); mis[(i + 1) % mis.length]?.focus() }
        if (e.key === 'ArrowUp') { e.preventDefault(); mis[(i - 1 + mis.length) % mis.length]?.focus() }
        if (e.key === 'Escape' || e.key === 'Tab') { state.menu = null; render() }
      },
    }, list.map(([label, fn, o = {}], i) => [
      o.danger && i ? h('div', { class: 'sep', role: 'separator' }) : null,
      h('button', {
        class: `mi${o.danger ? ' danger' : ''}`, type: 'button', role: 'menuitem', disabled: o.disabled, title: o.title,
        onclick: e => { e.stopPropagation(); state.menu = null; fn() },
      }, label),
    ])) : null)
}

// The menu everything shares: note, rotate, delete.
function commonItems(name, note) {
  const rotating = state.busy.has(`rotate:${name}`)
  const deleting = state.busy.has(`delete:${name}`)
  return [
    [note ? 'Edit note' : 'Add note', () => editNote(name), { title: 'Write what it is for, so you and your agents pick the right one' }],
    [rotating ? 'Rotating…' : 'Rotate', () => rotate(name), { disabled: rotating, title: 'Type a new value in a keymule dialog' }],
    [deleting ? 'Deleting…' : 'Delete', () => remove(name), { danger: true, disabled: deleting, title: 'Delete it from keymule (asks in a keymule window first)' }],
  ]
}

document.addEventListener('click', e => {
  if (state.menu && !e.target.closest('.menuwrap')) { state.menu = null; render() }
})

const isOpen = name => state.filter === name || state.describing === name

// Clicking a row opens it (details and note) and shows only its activity.
function rowAttrs(name) {
  const open = isOpen(name)
  return {
    class: `row click${open ? ' sel open' : ''}`,
    tabindex: '0',
    'aria-expanded': open ? 'true' : 'false',
    title: open ? 'Close' : 'Show details and only its activity',
    onclick: () => toggleRow(name),
    onkeydown: e => { if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); toggleRow(name) } },
  }
}

function toggleRow(name) {
  if (isOpen(name)) {
    state.filter = null
    if (state.describing === name) state.describing = null
  } else {
    state.filter = name
  }
  render()
}

const rowTitle = name => h('span', { class: 'title' }, h('span', { class: 'nm' }, name), h('span', { class: 'chev' }, icon('chev')))

// Shown when a row is open: the note in full, or a way to add one.
function details(name, text) {
  if (state.describing === name) return descLine(name, text)
  return h('span', { class: 'detail' },
    text ? h('span', { class: 'desc' }, text) : h('span', { class: 'desc none' }, 'No note yet.'),
    h('button', {
      class: 'btn notebtn', type: 'button',
      onclick: e => { e.stopPropagation(); editNote(name) },
      onkeydown: e => e.stopPropagation(),
    }, icon('pen'), text ? 'Edit note' : 'Add a note'))
}

// Long lists show their first `base` rows, then 20 more per click.
const STEP = 20
function paged(rows, key, base) {
  const extra = state.more[key] || 0
  const n = rows.length <= base + 1 ? rows.length : base + extra
  const out = rows.slice(0, n)
  const btn = (label, fn) => h('button', { class: 'more', type: 'button', onclick: () => { fn(); render() } }, label)
  const bar = [
    rows.length > n ? btn(`Show ${Math.min(STEP, rows.length - n)} more`, () => { state.more[key] = extra + STEP }) : null,
    extra ? btn('Show less', () => { state.more[key] = 0 }) : null,
  ].filter(Boolean)
  if (bar.length) out.push(h('div', { class: 'morebar' }, bar))
  return out
}
// The overview shows a few; a list's own page starts with 20.
const firstRows = n => (state.view === 'overview' ? n : STEP)

function descLine(name, text) {
  if (state.describing === name) {
    // Blur first: the page doesn't redraw a list while you type in it.
    const cancel = () => { input.blur(); state.describing = null; render() }
    const input = h('textarea', {
      class: 'field notefield', rows: 3, maxlength: 300, 'aria-label': `Description of ${name}`,
      placeholder: 'What it is for, e.g. Vultr VPS in Sydney (production)',
      onclick: e => e.stopPropagation(),
      onkeydown: e => {
        e.stopPropagation()
        if (e.key === 'Escape') cancel()
        // One line of text: Enter saves.
        if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); save() }
      },
    })
    input.value = text || ''
    const save = () => {
      input.blur()
      state.describing = null
      run(`describe:${name}`, 'Saving…', () => api('/api/describe', { name, description: input.value.replace(/\s+/g, ' ').trim() }))
    }
    setTimeout(() => { input.focus(); input.setSelectionRange(input.value.length, input.value.length) }, 0)
    return h('form', {
      class: 'inline', onclick: e => e.stopPropagation(),
      onsubmit: e => { e.preventDefault(); save() },
    }, input,
    h('span', { class: 'notefoot' },
      h('button', { class: 'btn primary', type: 'submit' }, 'Save note'),
      h('button', { class: 'btn ghost', type: 'button', onclick: cancel }, 'Cancel'),
      h('span', { class: 'muted' }, 'Enter to save · Esc to cancel')))
  }
  return text ? h('span', { class: 'desc' }, text) : null
}

// How passwords, keys and AWS pairs are used: only by programs, never HTTP.
const USE_HINT = {
  password: 'for programs: keymule exec --env VAR',
  'private-key': 'for programs, as a file: keymule exec --file KEY',
  aws: 'for programs: keymule exec (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)',
}

function credSub(c) {
  if (c.kind === 'ssh') {
    return c.ssh
      ? [mono(c.ssh.label), c.ssh.auth, mono(`keymule ssh ${c.name} -- <command>`)]
      : ['SSH login with no server set']
  }
  if (c.kind && c.kind !== 'http') return [c.service, USE_HINT[c.kind]]
  return [c.service, (c.hosts || []).join(', '), c.auth, ...(c.installed || []).map(i => `copied to ${i}`)]
}

function renderCreds(creds) {
  $('ncreds').textContent = creds.length || ''
  if (!creds.length) return [h('div', { class: 'empty' }, 'No API credentials yet. Add one below, or ask your agent: it opens the same dialog.')]
  return paged(creds.map(c => {
    const status = c.missing_secret ? badge('Missing', 'bad') : c.file_edited ? badge('Edited outside keymule', 'warn') : badge('Active', 'ok')
    const open = isOpen(c.name)
    return h('div', rowAttrs(c.name),
      ico(c.kind === 'ssh' ? 'terminal' : 'key'),
      h('span', { class: 'body' },
        rowTitle(c.name),
        h('span', { class: open ? 'sub wrap' : 'sub', title: open ? null : credSub(c).map(x => x.textContent ?? x).join(' · ') }, join(credSub(c))),
        open ? details(c.name, c.description) : null),
      h('span', { class: 'acc' }, status, revealButton(c.name), moreMenu(c.name, commonItems(c.name, c.description))))
  }), 'creds', firstRows(5))
}

function renderServers(servers) {
  $('nservers').textContent = servers.length || ''
  if (!servers.length) return [h('div', { class: 'empty' }, 'No MCP server secrets yet. Plain-text keys in your agent configs show up above, ready to move.')]
  return paged(servers.map(s => {
    const what = [...(s.env || []), ...(s.headers || []).map(x => `${x} header`)]
    if (s.arg_secrets) what.push(`${s.arg_secrets} in arguments`)
    const used = s.used_by.length
    const single = what.length === 1 && !s.arg_secrets
    const open = isOpen(s.name)
    const row = h('div', rowAttrs(s.name),
      ico('server'),
      h('span', { class: 'body' },
        rowTitle(s.name),
        h('span', { class: open ? 'sub wrap' : 'sub' }, join([mono(what.join(', ')), used ? `used by ${used} config entr${used === 1 ? 'y' : 'ies'}` : 'no MCP server runs it'])),
        open ? details(s.name, s.description) : null),
      h('span', { class: 'acc' }, used ? badge('Active', 'ok') : badge('Unused'), revealButton(s.name),
        moreMenu(s.name, [
          !used && single ? ['Move to Credentials', () => { state.converting = state.converting === s.name ? null : s.name; render() },
            { title: 'Not an MCP server secret? Make it an SSH login or a password, without retyping it' }] : null,
          ...commonItems(s.name, s.description),
        ])))
    return state.converting === s.name ? h('div', { class: 'rowgroup' }, row, convertForm(s)) : row
  }), 'servers', firstRows(5))
}

// Move an unused MCP server secret to Credentials as an SSH login or a
// password. The value never comes here: keymule moves it, after a yes in its
// own window.
function convertForm(s) {
  const f = state.convertForm ||= { to: 'ssh', host: '', username: '', port: '', description: s.description || '' }
  const field = (key, attrs) => h('input', {
    class: 'field', type: 'text', value: f[key], spellcheck: 'false', ...attrs,
    oninput: e => { f[key] = e.target.value },
  })
  const busy = state.busy.has(`convert:${s.name}`)
  const ssh = f.to === 'ssh'
  const to = h('select', { class: 'field', 'aria-label': 'Move it as', onchange: e => { f.to = e.target.value; render() } },
    h('option', { value: 'ssh', selected: ssh }, 'SSH login'),
    h('option', { value: 'password', selected: !ssh }, 'Password for programs'))
  return h('form', {
    class: 'convert',
    onsubmit: e => {
      e.preventDefault()
      if (ssh && (!f.host.trim() || !f.username.trim())) { toast('Give the server\'s host or IP and the username.', 'bad'); return }
      const body = { name: s.name, to: f.to, host: f.host.trim(), username: f.username.trim(), port: f.port.trim(), description: f.description.trim() }
      run(`convert:${s.name}`, `A keymule window asks before moving "${s.name}".`, () => api('/api/convert', body),
        () => { state.converting = null; state.convertForm = null })
    },
  },
  h('p', { class: 'note' }, `Move "${s.name}" to Credentials. keymule moves the value itself: nobody sees it or types it again.`),
  h('div', { class: 'addline' }, to,
    ssh ? field('host', { class: 'field url', placeholder: 'Host or IP, e.g. 203.0.113.7', 'aria-label': 'Host or IP' }) : null,
    ssh ? field('username', { class: 'field short', placeholder: 'Username', 'aria-label': 'Username' }) : null,
    ssh ? field('port', { class: 'field port', placeholder: '22', inputmode: 'numeric', 'aria-label': 'Port' }) : null),
  h('div', { class: 'addline' },
    field('description', { class: 'field url', placeholder: 'What it is for (optional)', 'aria-label': 'Description', maxlength: 300 }),
    h('button', { class: 'btn', type: 'submit', disabled: busy }, busy ? 'Waiting…' : 'Move'),
    h('button', { class: 'btn ghost', type: 'button', onclick: () => { state.converting = null; state.convertForm = null; render() } }, 'Cancel')))
}

// The kinds of use, in chart order (bottom to top). Colors: --s-<kind>.
const KINDS = [
  ['api', 'API call', 'API calls'],
  ['command', 'command', 'commands'],
  ['mcp', 'MCP server start', 'MCP server starts'],
  ['reveal', 'reveal', 'reveals'],
]
const KIND_TITLE = { api: 'API calls', command: 'Commands', mcp: 'MCP servers', reveal: 'Reveals' }
const plural = (n, one, many) => `${n.toLocaleString()} ${n === 1 ? one : many}`
const swatch = k => h('i', { class: `sw s-${k}`, 'aria-hidden': 'true' })

function renderStats() {
  const list = state.list
  if (!list) return []
  const t = state.stats?.daily?.at(-1)
  const n = list.credentials.length + list.servers.length
  const stat = (k, v, cls, sub) => h('div', { class: 'stat' }, h('span', { class: 'k' }, k), h('span', { class: `v ${cls || ''}` }, v), sub || null)
  const parts = t ? KINDS.filter(([k]) => t[k]).map(([k, one, many]) => h('span', { class: 'part' }, swatch(k), plural(t[k], one, many))) : []
  const sub = h('span', { class: 'statsub' }, parts.length ? join(parts) : 'Nothing yet today',
    h('button', { class: 'btn link', type: 'button', onclick: () => { go('usage'); render() } }, 'Usage'))
  return [
    stat('Secrets stored', n),
    stat('Uses today', t ? t.total.toLocaleString() : '—', '', sub),
    stat('Failed today', t ? t.failed : 0, t?.failed ? 'bad' : ''),
  ]
}

async function loadStats() {
  try {
    state.stats = await api(`/api/stats?days=${state.urange}&tz=${-new Date().getTimezoneOffset()}`)
    render()
  } catch {}
}

function setRange(n) {
  state.urange = n
  try { sessionStorage.setItem('keymule-urange', String(n)) } catch {}
  loadStats()
}

// ---- usage ---------------------------------------------------------------------

const NS = 'http://www.w3.org/2000/svg'
function sv(tag, attrs, ...kids) {
  const el = document.createElementNS(NS, tag)
  for (const [k, v] of Object.entries(attrs || {})) if (v != null) el.setAttribute(k, v)
  for (const kid of kids.flat(Infinity)) if (kid != null) el.append(kid instanceof Node ? kid : String(kid))
  return el
}

// Day numbers count days since 1970 in the viewer's time zone.
const dayDate = (d, opts) => new Date(d * 86400000).toLocaleDateString([], { timeZone: 'UTC', ...opts })
const dayLabel = (d, today) => d === today ? 'Today' : d === today - 1 ? 'Yesterday' : dayDate(d, { weekday: 'short', month: 'short', day: 'numeric' })

// A round top for the axis whose half is a whole number too.
function niceMax(v) {
  if (v <= 4) return 4
  const p = 10 ** Math.floor(Math.log10(v))
  return [1, 2, 3, 4, 5, 6, 8, 10].map(m => m * p).find(m => m >= v && m % 2 === 0)
}

// A bar with rounded top corners, square at the baseline.
function topRounded(x, y, w, hgt, r) {
  r = Math.min(r, w / 2, hgt)
  return `M${x},${y + hgt}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + hgt}Z`
}

function usageChart(s, width) {
  // Drawn at the real width, so text and bars stay their true size.
  const W = Math.max(300, width || 720), H = 220, L = 34, R = 6, T = 10, B = 24
  const pw = W - L - R, ph = H - T - B
  const days = s.daily
  const max = niceMax(Math.max(1, ...days.map(d => d.total)))
  const slot = pw / days.length
  const bw = Math.max(2, Math.min(28, slot * 0.64))
  const y = v => T + ph - (v / max) * ph
  const grid = [0, max / 2, max].map(v => [
    sv('line', { x1: L, x2: W - R, y1: y(v), y2: y(v), class: v ? 'ugrid' : 'ubase' }),
    sv('text', { x: L - 8, y: y(v) + 4, class: 'utick', 'text-anchor': 'end' }, Number.isInteger(v) ? v : v.toFixed(1)),
  ])
  const bars = days.map((d, i) => {
    const x = L + i * slot + (slot - bw) / 2
    let base = T + ph
    const segs = KINDS.filter(([k]) => d[k])
    return segs.map(([k], j) => {
      const hgt = (d[k] / max) * ph
      const top = base - hgt
      const gap = j > 0 && hgt > 3 ? 2 : 0 // a thin surface gap between stacked kinds
      const el = j === segs.length - 1
        ? sv('path', { d: topRounded(x, top, bw, hgt - gap, 3), class: `ubar s-${k}` })
        : sv('rect', { x, y: top, width: bw, height: Math.max(0, hgt - gap), class: `ubar s-${k}` })
      base = top
      return el
    })
  })
  const mid = Math.floor((days.length - 1) / 2)
  const xl = [[0, 'start'], [mid, 'middle'], [days.length - 1, 'end']].filter(([i], n, a) => a.findIndex(z => z[0] === i) === n)
  const xlabels = xl.map(([i, anchor]) => sv('text', {
    x: anchor === 'start' ? L : anchor === 'end' ? W - R : L + (i + 0.5) * slot, y: H - 6, class: 'utick', 'text-anchor': anchor,
  }, i === days.length - 1 ? 'Today' : dayDate(days[i].d, { month: 'short', day: 'numeric' })))
  const tip = h('div', { class: 'utip', hidden: true, role: 'status' })
  const hover = sv('rect', { class: 'uhover', x: 0, y: T, width: slot, height: ph, visibility: 'hidden' })
  const hits = days.map((d, i) => {
    const r = sv('rect', { x: L + i * slot, y: T, width: slot, height: ph, class: 'uhit' })
    r.addEventListener('pointerenter', () => {
      hover.setAttribute('x', L + i * slot); hover.setAttribute('visibility', 'visible')
      tip.replaceChildren(...[
        h('div', { class: 'utipd' }, dayLabel(d.d, s.today)),
        KINDS.map(([k]) => h('div', { class: `utipr${d[k] ? '' : ' zero'}` }, swatch(k), h('span', null, KIND_TITLE[k]), h('b', null, d[k]))),
        d.failed ? h('div', { class: 'utipr bad' }, h('span', null, 'Failed'), h('b', null, d.failed)) : null,
      ].flat().filter(Boolean))
      tip.hidden = false
      // Beside the bar: to its right, or to its left near the right edge.
      const wrap = tip.parentElement
      const px = ((L + (i + 0.5) * slot) / W) * wrap.clientWidth
      const right = px + slot / 2 + 10
      tip.style.left = `${right + tip.offsetWidth > wrap.clientWidth ? px - slot / 2 - 10 - tip.offsetWidth : right}px`
    })
    return r
  })
  const svg = sv('svg', { viewBox: `0 0 ${W} ${H}`, class: 'uchart', role: 'img', 'aria-label': `Uses per day for the last ${days.length} days, stacked by kind` },
    grid, hover, bars, xlabels, hits)
  svg.addEventListener('pointerleave', () => { tip.hidden = true; hover.setAttribute('visibility', 'hidden') })
  return h('div', { class: 'uchartwrap' }, svg, tip)
}

function usageTable(s) {
  return h('div', { class: 'utable', role: 'table', 'aria-label': 'Uses per day' },
    h('div', { class: 'utr head', role: 'row' }, h('span', { role: 'columnheader' }, 'Day'), KINDS.map(([k]) => h('span', { role: 'columnheader' }, KIND_TITLE[k])), h('span', { role: 'columnheader' }, 'Failed')),
    [...s.daily].reverse().map(d => h('div', { class: 'utr', role: 'row' },
      h('span', { role: 'cell' }, dayLabel(d.d, s.today)),
      KINDS.map(([k]) => h('span', { role: 'cell' }, d[k] || '·')),
      h('span', { role: 'cell', class: d.failed ? 't-bad' : null }, d.failed || '·'))))
}

function sparkline(values) {
  const W = 84, H = 22, max = Math.max(1, ...values)
  const pts = values.map((v, i) => `${(i / Math.max(1, values.length - 1)) * (W - 2) + 1},${H - 2 - (v / max) * (H - 4)}`).join(' ')
  return sv('svg', { viewBox: `0 0 ${W} ${H}`, class: 'uspark', 'aria-hidden': 'true' }, sv('polyline', { points: pts }))
}

function ago(ts) {
  if (!ts) return 'never'
  const s = Date.now() / 1000 - ts
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`
  if (s < 86400) return `${Math.round(s / 3600)} h ago`
  const d = Math.round(s / 86400)
  return d === 1 ? 'yesterday' : `${d} days ago`
}

function renderUsage() {
  const s = state.stats
  if (!s) return [h('div', { class: 'empty' }, 'Adding up keymule’s activity log…')]
  const sums = Object.fromEntries(KINDS.map(([k]) => [k, s.daily.reduce((a, d) => a + d[k], 0)]))
  const failed = s.daily.reduce((a, d) => a + d.failed, 0)
  const total = Object.values(sums).reduce((a, b) => a + b, 0)
  const range = h('div', { class: 'seg', role: 'group', 'aria-label': 'Time range' },
    [7, 30, 90].map(n => h('button', { type: 'button', class: s.days === n ? 'on' : null, 'aria-pressed': String(s.days === n), onclick: () => setRange(n) }, `${n} days`)))
  const legend = h('div', { class: 'ulegend' },
    KINDS.map(([k]) => h('div', { class: 'ul' }, h('span', { class: 'k' }, swatch(k), KIND_TITLE[k]), h('span', { class: 'v' }, sums[k].toLocaleString()))),
    h('div', { class: 'ul' }, h('span', { class: 'k' }, 'Failed'), h('span', { class: `v ${failed ? 't-bad' : ''}` }, failed.toLocaleString())))
  const used = s.keys.filter(k => k.total)
  const known = [...(state.list?.credentials || []), ...(state.list?.servers || [])].map(c => c.name)
  const byName = Object.fromEntries(s.keys.map(k => [k.name, k]))
  const idle = known.filter(n => !byName[n]?.total).map(n => ({ name: n, last: byName[n]?.last || 0 })).sort((a, b) => b.last - a.last)
  return [
    h('div', { class: 'ush' }, h('h3', null, `${total.toLocaleString()} uses in the last ${s.days} days`), h('span', { class: 'grow' }), range),
    legend,
    total ? usageChart(s, $('usage').clientWidth) : h('div', { class: 'empty' }, `No key was used in the last ${s.days} days.`),
    total ? h('div', { class: 'umore' },
      h('button', { class: 'btn link', type: 'button', onclick: () => { state.utable = !state.utable; render() } }, state.utable ? 'Hide the table' : 'Show as a table'),
      h('span', { class: 'muted' }, 'Commands and server starts are counted when they start, so only API calls and GitHub uploads can fail here.')) : null,
    state.utable && total ? usageTable(s) : null,
    h('div', { class: 'ush' }, h('h3', null, 'By key')),
    used.length ? h('div', { class: 'ukeys' }, used.map(k => h('div', { class: 'ukey' },
      h('span', { class: 'nm mono' }, k.name),
      sparkline(k.spark),
      h('span', { class: 'split' }, join(KINDS.filter(([c]) => k[c]).map(([c, one, many]) => h('span', { class: 'part' }, swatch(c), plural(k[c], one, many))))),
      h('span', { class: 'tot' }, k.total.toLocaleString(), k.failed ? h('span', { class: 't-bad' }, ` · ${k.failed} failed`) : null),
      h('span', { class: 'last' }, ago(k.last)))))
      : h('div', { class: 'empty' }, 'No key was used in this range.'),
    idle.length ? [
      h('div', { class: 'ush' }, h('h3', null, `Not used in the last ${s.days} days`), h('span', { class: 'count' }, idle.length)),
      h('p', { class: 'note' }, 'Stored but idle. If you no longer need one, rotate or remove it: a key nobody uses is still a key that can leak.'),
      h('div', { class: 'ukeys idle' }, idle.map(k => h('div', { class: 'ukey' }, h('span', { class: 'nm mono' }, k.name), h('span', { class: 'last' }, k.last ? `last used ${ago(k.last)}` : 'never used')))),
    ] : null,
  ].flat().filter(Boolean)
}

// A newer release, or an upgrade this page's keymule hasn't restarted onto.
// Quiet on purpose: everything still works. keymule never updates itself.
function renderUpdate() {
  const u = state.update
  if (!u) return []
  const copyCmd = cmd => h('button', {
    class: 'btn', type: 'button',
    onclick: async e => {
      try { await navigator.clipboard.writeText(cmd); e.target.textContent = 'Copied' } catch { e.target.textContent = 'Couldn’t copy' }
    },
  }, 'Copy')
  if (u.replaced) {
    const menubar = state.host === 'menubar'
    return [h('div', { class: 'pairing slim update' },
      h('div', { class: 'pairtext' },
        h('strong', null, 'keymule was updated. Restart it to use the new version'),
        h('span', null, menubar
          ? 'Restart your agents too (Claude Code, Codex, Cursor), so the keymule they started is the new one.'
          : 'Stop `keymule ui` and start it again. Restart your agents too, so the keymule they started is the new one.')),
      menubar ? h('div', { class: 'pairgo' }, h('button', {
        class: 'btn primary', type: 'button',
        onclick: async e => {
          e.target.disabled = true; e.target.textContent = 'Restarting…'
          try { await api('/api/restart', {}) } catch {}
        },
      }, 'Restart now')) : null)]
  }
  if (!u.latest) return []
  return [h('div', { class: 'pairing slim update' },
    h('div', { class: 'pairtext' },
      h('strong', null, `keymule ${u.latest} is out`),
      h('span', null, `You have ${u.current}. `, h('a', { href: u.notes, target: '_blank', rel: 'noopener' }, 'What’s new'), '. To update, run:'),
      h('code', { class: 'updcmd' }, u.command)),
    h('div', { class: 'pairgo' }, copyCmd(u.command)))]
}

function renderBanner() {
  // No keychain and no passphrase: nothing can be saved, so say how to fix it first.
  const vp = state.list?.vault_problem
  if (vp) {
    const [why, ...help] = vp.split('\n\n')
    return [h('div', { class: 'callout' },
      ico('warn'),
      h('div', { class: 'body' },
        h('span', { class: 'title' }, 'keymule can’t store secrets on this machine yet'),
        h('span', { class: 'sub' }, `${why}. Set a passphrase where your agents run, then restart them:`),
        h('pre', { class: 'plan' }, help.join('\n\n'))))]
  }
  const p = state.plan
  if (!p || !p.has) return []
  const busy = state.busy.has('adopt')
  return [h('div', { class: 'callout' },
    ico(busy ? 'spin' : 'warn'),
    h('div', { class: 'body' },
      h('span', { class: 'title' }, p.summary),
      h('span', { class: 'sub' }, `${p.names.join(', ')}: keymule can move them into the keychain, and the servers keep working.`),
      h('div', { class: 'acts' },
        h('button', { class: 'btn primary', type: 'button', disabled: busy, onclick: applyPlan }, state.confirmApply ? 'Click again to confirm' : 'Move to keychain'),
        h('button', { class: 'btn', type: 'button', onclick: () => { state.planOpen = !state.planOpen; render() } }, state.planOpen ? 'Hide details' : 'Show details')),
      state.planOpen ? h('pre', { class: 'plan' }, p.text) : null))]
}

const SEVERITY = { leaked: 0, plaintext: 1, readable: 2 }
const SEV_LABEL = { leaked: 'sent to a model', plaintext: 'plain text in a config', readable: 'readable by agents' }

function renderLeaks() {
  const d = state.doctor
  $('scan').textContent = d === 'running' ? 'Scanning…' : d ? 'Scan again' : 'Scan'
  $('scan').disabled = d === 'running'
  if (d === null) return [h('div', { class: 'scanbox' }, ico('shield'), h('span', {}, 'Look for keys that leaked into agent transcripts, sit in plain text in configs, or are readable in .env files and shell history. Values stay masked.'))]
  if (d === 'running') return [scanBox()]
  if (d.error) return [h('div', { class: 'scanbox' }, ico('x'), h('span', { class: 't-bad' }, d.error))]

  const worst = e => e.locations.map(l => l.severity).sort((a, b) => SEVERITY[a] - SEVERITY[b])[0]
  const entries = [...d.entries].sort((a, b) => SEVERITY[worst(a)] - SEVERITY[worst(b)])
  const count = sev => entries.filter(e => worst(e) === sev).length
  const stored = entries.filter(e => e.stored_as).length
  const st = d.stats || {}
  const c = (k, v, cls) => h('div', {}, h('span', { class: 'k' }, k), h('span', { class: `v ${v ? cls : ''}` }, v))
  const counts = h('div', { class: 'counts' },
    c('Leaked', count('leaked'), 't-bad'), c('Plain text', count('plaintext'), 't-warn'), c('Readable', count('readable'), ''), c('In keymule', stored, ''))
  if (!entries.length) {
    return [counts, h('div', { class: 'empty' }, `Nothing found in ${st.transcripts ?? 0} transcripts, ${(st.configs || []).length} configs and ${st.env_files ?? 0} .env files.`)]
  }
  const rows = paged(entries, 'leaks', 10).map(e => {
    if (e instanceof Node) return e
    const sev = worst(e)
    const first = e.locations[0] || {}
    const more = e.locations.length > 1 ? `+${e.locations.length - 1} more` : null
    const b = e.stored_as ? badge('In keymule', 'ok') : sev === 'leaked' ? badge('Rotate it', 'bad') : sev === 'plaintext' ? badge('Move it', 'warn') : badge('Readable')
    return h('div', { class: 'row', title: e.locations.map(l => `${l.place}${l.date ? ' · ' + l.date : ''}`).join('\n') },
      ico(sev === 'leaked' ? 'warn' : 'eye'),
      h('span', { class: 'body' },
        h('span', { class: 'title' }, mono(e.masked)),
        h('span', { class: 'sub' }, join([e.label, SEV_LABEL[sev], first.place, first.date, more]))),
      h('span', { class: 'acc' }, b))
  })
  return [counts, ...rows]
}

// Audit lines that aren't HTTP requests: what to call them.
const EVENTS = {
  START: 'Started', EXEC: 'Ran', INSTALL: 'Copied to git', UNINSTALL: 'Removed from git',
  'GH-SECRET': 'Copied to GitHub', 'AUTO-ALLOW': 'Allowed', 'ALLOW-ALL-ON': 'Allow all on',
  'ALLOW-ALL-OFF': 'Allow all off', PAIR: 'Paired', 'BAD-APPROVAL': 'Refused',
  'PAGE-LOGIN': 'Signed in', 'ANSWERED-IN-PAGE': 'Answered', 'DENIED-IN-PAGE': 'Denied',
  SSH: 'Logged in', CONVERT: 'Moved', RENAME: 'Renamed',
}
// keymule's own events have no credential: show them as keymule's.
const actName = a => (a.name === '-' ? 'keymule' : a.name)

function renderActivity() {
  const f = state.filter
  $('filter').replaceChildren(f ? h('button', { class: 'chip', type: 'button', onclick: () => toggleFilter(f), title: 'Show everything' }, f, ' ×') : '')
  const all = state.audit.filter(a => !f || a.name === f).slice(-1000).reverse()
  if (!all.length) return [h('div', { class: 'empty' }, f ? `Nothing from ${f} yet.` : 'Requests and server starts show up here as they happen.')]
  return paged(all, 'act', state.view === 'activity' ? 50 : 8).map(a => {
    if (a instanceof Node) return a
    const event = EVENTS[a.method]
    const shown = a.method === 'REVEAL'
    const ok = a.status >= 200 && a.status < 300
    return h('div', { class: 'act' },
      h('span', { class: 'name', onclick: () => toggleFilter(a.name), title: 'Show only its activity' }, actName(a)),
      h('span', { class: 'req', title: a.url }, h('span', { class: 'm' }, a.method), h('span', { class: 'u' }, shown ? 'shown to you in a keymule window' : a.url)),
      h('span', { class: 'st' }, event ? badge(event, a.method === 'BAD-APPROVAL' ? 'bad' : '') : shown ? badge('Revealed', 'warn') : badge(a.status || '—', ok ? 'ok' : 'bad')),
      h('span', { class: 'when' }, when(a.ts)))
  })
}

function renderNav() {
  const list = state.list
  const d = state.doctor
  const leaks = d && d !== 'running' && d.entries ? d.entries.filter(e => !e.stored_as).length : null
  const counts = { credentials: list?.credentials.length, servers: list?.servers.length, leaks }
  $('nav').replaceChildren(...VIEWS.map(([id, label, ic]) => h('button', {
    type: 'button', class: state.view === id ? 'on' : null, 'aria-current': state.view === id ? 'page' : null,
    onclick: () => go(id), title: collapsed ? label : null,
  }, icon(ic), h('span', { class: 'label' }, label), counts[id] != null ? h('span', { class: `n ${id === 'leaks' && counts[id] ? 'bad' : ''}` }, counts[id]) : null)))
  const v = VIEWS.find(x => x[0] === state.view) || VIEWS[0]
  $('ptitle').textContent = v[1]
  $('psub').textContent = v[3]
  $('main').className = `main v-${v[0]}`
  for (const el of document.querySelectorAll('[data-in]')) el.hidden = !el.dataset.in.split(' ').includes(v[0])
}

const drawn = {}
const objId = new WeakMap()
let nextId = 1
const idOf = o => (o && typeof o === 'object' ? (objId.get(o) || (objId.set(o, nextId++), objId.get(o))) : o)
function draw(id, key, build) {
  const k = JSON.stringify(key)
  if (drawn[id] === k) return
  drawn[id] = k
  $(id).replaceChildren(...build())
}

function render() {
  const y = window.scrollY
  renderNav()
  const live = $('live')
  live.className = `live${state.live === false ? ' off' : ''}`
  if (state.live === false) {
    live.replaceChildren(h('span', { class: 'dot' }), state.forbidden
      ? 'No access: open the link `keymule ui` printed'
      : 'Disconnected: run `keymule ui` again')
  } else if (state.live) {
    live.replaceChildren(h('span', { class: 'dot' }), 'Live')
  }
  $('version').textContent = state.version ? `v${state.version}` : ''
  const list = state.list
  $('stats').replaceChildren(...renderStats())
  $('banner').replaceChildren(...renderBanner())
  draw('update', [state.update, state.host], renderUpdate)
  const typing = el => el.contains(document.activeElement) && document.activeElement.matches('input, select, textarea, .mi')
  const busy = [...state.busy]
  const rowState = [state.view, state.filter, state.describing, state.menu, busy]
  if (list) {
    if (!typing($('creds'))) draw('creds', [list.credentials, state.more.creds, ...rowState], () => renderCreds(list.credentials))
    if (!typing($('servers'))) draw('servers', [list.servers, state.more.servers, state.converting, state.convertForm?.to, ...rowState], () => renderServers(list.servers))
  } else if (state.error) {
    draw('creds', ['error', state.error], () => [h('div', { class: 'empty t-bad' }, state.error)])
  }
  $('add').querySelector('button').disabled = state.busy.has('add')
  draw('leaks', [idOf(state.doctor), state.more.leaks, state.doctor === 'running' ? state.scan?.id : null], renderLeaks)
  if (state.doctor === 'running') updateScanBox()
  const last = state.audit[state.audit.length - 1]
  draw('activity', [state.audit.length, last?.ts, last?.name, state.filter, state.more.act, state.view, new Date().toDateString()], renderActivity)
  if (state.view === 'usage') draw('usage', [idOf(state.stats), state.utable, state.list?.credentials.length, state.list?.servers.length, Math.round($('usage').clientWidth / 40)], renderUsage)
  // Nothing above should move the page; put it back if the browser did.
  if (Math.abs(window.scrollY - y) > 1) window.scrollTo(0, y)
  updateProgress()
}

// Scroll position as a thin bar along the top, in place of a scrollbar.
function updateProgress() {
  const s = document.scrollingElement
  const max = s.scrollHeight - s.clientHeight
  $('progress').firstElementChild.style.transform = `scaleX(${max > 0 ? Math.min(1, s.scrollTop / max) : 0})`
}
addEventListener('scroll', updateProgress, { passive: true })
addEventListener('resize', updateProgress)
let resizeTimer
addEventListener('resize', () => { clearTimeout(resizeTimer); resizeTimer = setTimeout(render, 150) })

render()
refresh()
loadStats()
loadPlan()
pollScan()
setInterval(refresh, 20000)
let statsKey = ''
setInterval(async () => {
  try {
    const r = await api('/api/audit')
    state.audit = r.audit || []
    state.live = true
    const last = state.audit[state.audit.length - 1]
    const key = `${state.audit.length}|${last?.ts}|${new Date().toDateString()}`
    if (key !== statsKey) { statsKey = key; loadStats() }
  } catch (e) {
    state.live = false
    state.forbidden = e.message === 'forbidden'
  }
  render()
}, 3000)
setInterval(loadPlan, 60000)

// ---- questions keymule asks here (no screen for a popup) ----------------------
//
// Each browser makes its own key pair (the private half can't be read out,
// even by this page) and keeps it in IndexedDB. Pairing hands keymule the
// public half; every answer is signed with the private one, so nothing else
// can answer for you. A typed secret is encrypted to the asking process.

const ap = { pending: [], devices: [], pageMode: false, me: null, live: false, busy: false, err: '', value: '', code: '', revealKeys: {}, version: '', hidden: new Set() }

const enc = s => new TextEncoder().encode(s)
const b64 = buf => btoa(String.fromCharCode(...new Uint8Array(buf)))
const unb64 = s => Uint8Array.from(atob(s), c => c.charCodeAt(0))
const hexOf = buf => [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join('')
const sha256 = data => crypto.subtle.digest('SHA-256', typeof data === 'string' ? enc(data) : data)

function idb(mode, fn) {
  return new Promise((resolve, reject) => {
    const open = indexedDB.open('keymule', 1)
    open.onupgradeneeded = () => open.result.createObjectStore('device')
    open.onerror = () => reject(open.error)
    open.onsuccess = () => {
      const tx = open.result.transaction('device', mode)
      const req = fn(tx.objectStore('device'))
      req.onsuccess = () => resolve(req.result)
      req.onerror = () => reject(req.error)
    }
  })
}

async function myDevice() {
  if (ap.me) return ap.me
  let d = await idb('readonly', s => s.get('me')).catch(() => null)
  if (!d) {
    const keyPair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])
    const raw = await crypto.subtle.exportKey('raw', keyPair.publicKey)
    d = { keyPair, pub: b64(raw), id: hexOf((await sha256(raw)).slice(0, 8)) }
    await idb('readwrite', s => s.put(d, 'me'))
  }
  ap.me = d
  return d
}

function browserName() {
  const ua = navigator.userAgent
  const b = /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : 'Browser'
  const os = /Mac OS X/.test(ua) ? 'Mac' : /Windows/.test(ua) ? 'Windows' : /Linux/.test(ua) ? 'Linux' : ''
  return os ? `${b} on ${os}` : b
}

const paired = () => !!ap.me && ap.devices.some(d => d.id === ap.me.id)

// With `wait`, the server holds the request until something changes (up to
// 25 s). Chrome slows timers in background tabs to once a minute, but not
// an answer to a request, so a question still shows (and notifies) at once.
async function loadApprovals(wait) {
  try {
    const r = await api(`/api/approvals${wait ? `?wait=${ap.version}` : ''}`)
    ap.version = r.version || ''
    ap.pending = (r.pending || []).map(raw => ({ raw, req: JSON.parse(raw) }))
    const ids = new Set(ap.pending.map(p => p.req.id))
    for (const id of ap.hidden) if (!ids.has(id)) ap.hidden.delete(id)
    ap.devices = r.devices || []
    ap.pageMode = !!r.page_mode
    ap.live = true
    if (ap.pageMode || ap.devices.length) await myDevice()
    for (const sh of r.shows || []) {
      const k = ap.revealKeys[sh.id]
      if (!k) continue
      delete ap.revealKeys[sh.id]
      openShow(sh, k).then(showRevealed).catch(e => toast(`Couldn’t open the value: ${e.message}`, 'bad'))
    }
  } catch {
    ap.live = false
  }
  renderApprovals()
  return ap.live
}

async function watchApprovals() {
  for (;;) {
    if (!await loadApprovals(true)) await new Promise(r => setTimeout(r, 2000))
  }
}

async function pairThis(code) {
  ap.busy = true; ap.err = ''; renderApprovals()
  try {
    const d = await myDevice()
    const r = await api('/api/pair', { name: browserName(), key: d.pub, code })
    if (!r.ok) ap.err = r.stderr || 'Pairing failed'
    else { ap.code = ''; toast('Paired. This browser answers keymule’s questions and opens the page without the link', 'ok') }
  } catch (e) {
    ap.err = e.message
  }
  ap.busy = false
  await loadApprovals()
}

async function sealSecret(req, value) {
  const asker = await crypto.subtle.importKey('raw', unb64(req.epk), { name: 'ECDH', namedCurve: 'P-256' }, false, [])
  const eph = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits'])
  const bits = new Uint8Array(await crypto.subtle.deriveBits({ name: 'ECDH', public: asker }, eph.privateKey, 256))
  const prefix = enc(`keymule-secret-v1\n${req.id}\n`)
  const both = new Uint8Array(prefix.length + bits.length)
  both.set(prefix); both.set(bits, prefix.length)
  const aes = await crypto.subtle.importKey('raw', await sha256(both), 'AES-GCM', false, ['encrypt'])
  const iv = crypto.getRandomValues(new Uint8Array(12))
  const ct = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, aes, enc(value))
  return { epk: b64(await crypto.subtle.exportKey('raw', eph.publicKey)), iv: b64(iv), ct: b64(ct) }
}

// A value keymule encrypted for this tab after you said Show.
async function openShow(sh, k) {
  const asker = await crypto.subtle.importKey('raw', unb64(k.asker), { name: 'ECDH', namedCurve: 'P-256' }, false, [])
  const bits = new Uint8Array(await crypto.subtle.deriveBits({ name: 'ECDH', public: asker }, k.priv, 256))
  const prefix = enc(`keymule-show-v1\n${sh.id}\n`)
  const both = new Uint8Array(prefix.length + bits.length)
  both.set(prefix); both.set(bits, prefix.length)
  const aes = await crypto.subtle.importKey('raw', await sha256(both), 'AES-GCM', false, ['decrypt'])
  const plain = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: unb64(sh.iv) }, aes, unb64(sh.ct))
  return JSON.parse(new TextDecoder().decode(plain))
}

let revealTimer
function closeRevealed() {
  clearInterval(revealTimer)
  const wrap = $('reveal')
  wrap.hidden = true
  wrap.replaceChildren()
}
function showRevealed(data) {
  const wrap = $('reveal')
  const until = Date.now() + 30000
  const left = h('span', { class: 'muted' })
  const tick = () => {
    const s = Math.max(0, Math.ceil((until - Date.now()) / 1000))
    left.textContent = `Closes in ${s} s`
    if (!s) closeRevealed()
  }
  const copyBtn = data.copy ? h('button', {
    class: 'btn', type: 'button',
    onclick: async () => {
      try { await navigator.clipboard.writeText(data.copy); copyBtn.textContent = 'Copied' } catch { copyBtn.textContent = 'Couldn’t copy' }
    },
  }, 'Copy') : null
  wrap.replaceChildren(h('div', { class: 'ask', role: 'dialog', 'aria-modal': 'true', 'aria-label': `${data.name}, shown for 30 seconds` },
    h('div', { class: 'askbrand' }, 'keymule · only this browser can read this'),
    h('div', { class: 'askmain' },
      h('span', { class: 'askicon' }, icon('eye')),
      h('div', { class: 'asktext' },
        h('h2', {}, data.name),
        (data.items || []).map(([k, v]) => [h('p', { class: 'asklabel' }, k), h('pre', { class: 'askcode revealval' }, v)]))),
    h('div', { class: 'askfoot' }, left, h('span', { class: 'grow' }), copyBtn,
      h('button', { class: 'btn primary', type: 'button', onclick: closeRevealed }, 'Done'))))
  wrap.hidden = false
  tick()
  clearInterval(revealTimer)
  revealTimer = setInterval(tick, 1000)
  wrap.querySelector('.btn.primary').focus()
}
document.addEventListener('keydown', e => { if (e.key === 'Escape' && !$('reveal').hidden) closeRevealed() })

async function answer(item, button) {
  const { req, raw } = item
  const s = req.spec
  ap.busy = true; ap.err = ''; renderApprovals()
  try {
    const d = await myDevice()
    let secret = null
    if (s.secure && button !== s.cancel) {
      if (!ap.value.trim()) { ap.err = 'Type the secret first'; ap.busy = false; renderApprovals(); return }
      secret = await sealSecret(req, ap.value)
    }
    if (s.reveal && button !== s.cancel) {
      // A one-off key for keymule to encrypt the value to. Its private half
      // stays in this tab (not extractable) and is dropped once used.
      const eph = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits'])
      secret = { epk: b64(await crypto.subtle.exportKey('raw', eph.publicKey)), iv: '', ct: '' }
      ap.revealKeys[req.id] = { priv: eph.privateKey, asker: req.epk }
    }
    const sec = secret ? hexOf(await sha256(`${secret.epk}.${secret.iv}.${secret.ct}`)) : ''
    const msg = `keymule-approval-v1\n${req.id}\n${hexOf(await sha256(raw))}\n${button}\n${sec}`
    const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, d.keyPair.privateKey, enc(msg))
    const r = await api('/api/approvals/answer', { id: req.id, reply: { button, device: d.id, sig: b64(sig), secret } })
    if (!r.ok) ap.err = r.stderr || 'That didn’t go through'
    else { ap.value = ''; ap.pending = ap.pending.filter(p => p.req.id !== req.id) }
  } catch (e) {
    ap.err = e.message
  }
  ap.busy = false
  renderApprovals()
}

// Saying no needs no signature, so a browser that isn't paired can do it
// too and is never stuck behind a question.
async function deny(item) {
  ap.busy = true; ap.err = ''; renderApprovals()
  try {
    const r = await api('/api/approvals/deny', { id: item.req.id })
    if (!r.ok) ap.err = r.stderr || 'That didn’t go through'
    else { ap.value = ''; ap.pending = ap.pending.filter(p => p.req.id !== item.req.id) }
  } catch (e) {
    ap.err = e.message
  }
  ap.busy = false
  renderApprovals()
}

// Close the question in this browser only; it keeps waiting.
function hideAsk(item) {
  ap.hidden.add(item.req.id)
  ap.err = ''
  renderApprovals()
}

// The question, laid out like keymule's popup: headline, text, code boxes.
function askBody(body) {
  const paras = (body || '').split(/\n\n+/).filter(p => p.trim())
  return paras.map((p, i) => {
    const lines = p.split('\n')
    if (lines.every(l => l.startsWith('        '))) return h('pre', { class: 'askcode' }, lines.map(l => l.slice(8)).join('\n'))
    const next = paras[i + 1]
    const label = next && next.split('\n').every(l => l.startsWith('        ')) && p.length < 60
    return h('p', { class: label ? 'asklabel' : null }, p)
  })
}

// The field for a secret: one line, or several (a private key, AWS keys),
// hidden either way. Enter saves one line; ⌘/Ctrl-Enter saves several.
function secretField(item, s) {
  const el = h(s.multiline ? 'textarea' : 'input', {
    id: 'askval', class: `field askval${s.multiline ? ' multi' : ''}`,
    type: s.multiline ? null : 'password', autocomplete: 'off', spellcheck: 'false',
    placeholder: s.multiline ? 'Paste it here (⌘↩ saves)' : 'Paste or type it here',
    oninput: e => { ap.value = e.target.value },
    onkeydown: e => {
      if (e.key === 'Enter' && (!s.multiline || e.metaKey || e.ctrlKey)) { e.preventDefault(); answer(item, s.default) }
    },
  })
  el.value = ap.value
  // Hidden again for each new question; the eye shows what was typed.
  if (ap.peekId !== item.req.id) { ap.peekId = item.req.id; ap.peek = false }
  const peek = h('button', {
    class: 'peek', type: 'button',
    onmousedown: e => e.preventDefault(), // the cursor stays in the field
    onclick: () => { ap.peek = !ap.peek; draw() },
  })
  const draw = () => {
    peek.replaceChildren(icon(ap.peek ? 'eyeOff' : 'eye'))
    peek.title = ap.peek ? 'Hide' : 'Show'
    peek.setAttribute('aria-label', ap.peek ? 'Hide the value' : 'Show the value')
    peek.setAttribute('aria-pressed', String(ap.peek))
    if (s.multiline) el.classList.toggle('shown', ap.peek)
    else el.type = ap.peek ? 'text' : 'password'
  }
  draw()
  return h('div', { class: 'askvalwrap' }, el, peek)
}

const ASK_ICON = { key: 'key', unlock: 'shield', trash: 'x', eye: 'eye', shield: 'shield' }

let askKey = ''
function renderAsk() {
  const wrap = $('ask')
  const shown = ap.pending.filter(p => !ap.hidden.has(p.req.id))
  const item = shown[0]
  const me = paired()
  // Redraw only when something changed: a redraw mid-click would lose it.
  const key = item && ap.live ? [item.req.id, shown.length, ap.err, ap.busy, me, notifyAsk()].join('|') : ''
  if (key === askKey) return
  askKey = key
  if (!key) { wrap.hidden = true; wrap.replaceChildren(); return }
  const s = item.req.spec
  const focusHad = document.activeElement && ['askval', 'askcode'].includes(document.activeElement.id)
  const noLabel = s.cancel === 'Cancel' ? 'Deny' : s.cancel
  const card = h('div', { class: `ask ${s.tone || ''}`, role: 'dialog', 'aria-modal': 'true', 'aria-labelledby': 'asktitle' },
    h('div', { class: 'askbrand' }, 'keymule asks',
      shown.length > 1 ? h('span', { class: 'askmore' }, `+${shown.length - 1} more`) : null),
    h('div', { class: 'askmain' },
      h('div', { class: 'askicon' }, icon(ASK_ICON[s.icon] || 'key')),
      h('div', { class: 'asktext' },
        h('h2', { id: 'asktitle' }, s.title),
        askBody(s.body),
        s.secure && me ? secretField(item, s) : null,
        me ? null : pairInCard(),
        ap.err ? h('p', { class: 'askerr' }, ap.err) : null)),
    notifyAsk() ? h('div', { class: 'asknotify' }, notifyButton('Get a notification next time')) : null,
    me
      ? h('div', { class: 'askfoot' }, s.buttons.map((b, i) => [
          h('button', {
            class: `btn${b === s.destructive ? ' danger-fill' : b === s.primary ? ' primary' : ''}`,
            type: 'button', disabled: ap.busy, 'data-safe': b === s.cancel ? '1' : null,
            onclick: () => answer(item, b),
          }, b),
          i === 0 && s.buttons.length > 2 ? h('span', { class: 'grow' }) : null,
        ]))
      : h('div', { class: 'askfoot' },
          h('button', { class: 'btn', type: 'button', disabled: ap.busy, onclick: () => hideAsk(item) }, 'Hide'),
          h('span', { class: 'grow' }),
          h('button', { class: 'btn', type: 'button', disabled: ap.busy, 'data-safe': '1', onclick: () => deny(item) }, noLabel)))
  wrap.replaceChildren(card)
  wrap.hidden = false
  const val = $('askval') || $('askcode')
  if (val && (focusHad || wrap.dataset.shown !== item.req.id)) val.focus()
  // Focus the safe button, never the one that says yes: a question can wait
  // in a tab for minutes, and a stray Space or Return would press it.
  else if (!val && wrap.dataset.shown !== item.req.id) wrap.querySelector('[data-safe]')?.focus()
  wrap.dataset.shown = item.req.id
}

// Shown in the question when this browser can't answer yet: pair right here.
function pairInCard() {
  const go = () => pairThis(ap.code)
  const input = h('input', {
    id: 'askcode', class: 'field paircode', inputmode: 'numeric', placeholder: '123-456', 'aria-label': 'Pairing code',
    oninput: e => { ap.code = e.target.value },
    onkeydown: e => { if (e.key === 'Enter') { e.preventDefault(); go() } },
  })
  input.value = ap.code
  return h('div', { class: 'askpair' },
    h('p', null, 'This browser isn’t paired, so it can only say no. To answer, enter the code `keymule ui` printed in your terminal.'),
    ap.devices.length ? h('p', { class: 'muted' }, 'Paired before? Use the same address: 127.0.0.1 and localhost count as different.') : null,
    h('div', { class: 'pairgo' }, input,
      h('button', { class: 'btn primary', type: 'button', disabled: ap.busy, onclick: go }, ap.busy ? 'Pairing…' : 'Pair')))
}

// ---- notifications and the tab title -------------------------------------------
//
// keymule's questions wait in this page and time out after 3 minutes, so tell
// the user even when they're looking at a terminal: a system notification
// (the browser's) and a count in the tab title.

const canNotify = () => 'Notification' in window && window.isSecureContext
const usesPage = () => ap.pageMode || ap.devices.length > 0
let notifyLater = false
try { notifyLater = localStorage.getItem('keymule-notify-later') === '1' } catch {}
const notifyAsk = () => canNotify() && usesPage() && Notification.permission === 'default'

function notifyButton(label) {
  return h('button', {
    class: 'btn link', type: 'button',
    onclick: async () => {
      try { await Notification.requestPermission() } catch {}
      if (Notification.permission === 'granted') toast('You’ll get a notification when keymule asks', 'ok')
      askKey = ''; pairKey = ''; renderApprovals()
    },
  }, label)
}

const notes = new Map()
function notifyNew() {
  const ids = new Set(ap.pending.map(p => p.req.id))
  for (const [id, n] of notes) if (!ids.has(id)) { n?.close(); notes.delete(id) }
  if (!canNotify() || Notification.permission !== 'granted') return
  for (const p of ap.pending) {
    if (notes.has(p.req.id)) continue
    if (document.visibilityState === 'visible' && document.hasFocus()) { notes.set(p.req.id, null); continue }
    try {
      const n = new Notification('keymule asks', { body: p.req.spec.title, tag: `keymule-${p.req.id}`, requireInteraction: true })
      n.onclick = () => { window.focus(); ap.hidden.delete(p.req.id); renderApprovals(); n.close() }
      notes.set(p.req.id, n)
    } catch { notes.set(p.req.id, null) }
  }
}

const baseTitle = document.title
function renderTitle() {
  const n = ap.pending.length
  document.title = n ? `(${n}) keymule asks` : baseTitle
}

let pairKey = ''
function renderPairing() {
  const el = $('pairing')
  const show = ap.live && usesPage() && !paired()
  const first = ap.devices.length === 0
  const waiting = ap.pending.filter(p => ap.hidden.has(p.req.id)).length
  const nudge = ap.live && notifyAsk() && !notifyLater
  const key = [show, first, ap.busy, ap.pending.length ? '' : ap.err, waiting, nudge].join('|')
  if (key === pairKey) return
  pairKey = key
  const parts = []
  if (waiting) {
    parts.push(h('div', { class: 'pairing slim' },
      h('div', { class: 'pairtext' }, h('strong', null, waiting === 1 ? 'A question is waiting for you' : `${waiting} questions are waiting for you`)),
      h('div', { class: 'pairgo' }, h('button', { class: 'btn primary', type: 'button', onclick: () => { ap.hidden.clear(); renderApprovals() } }, 'Show'))))
  }
  if (show) {
    const input = h('input', { id: 'paircode', class: 'field paircode', inputmode: 'numeric', placeholder: '123-456', 'aria-label': 'Pairing code' })
    const go = () => pairThis($('paircode')?.value || '')
    input.addEventListener('keydown', e => { if (e.key === 'Enter') go() })
    parts.push(h('div', { class: 'pairing' },
      h('div', { class: 'pairtext' },
        h('strong', null, 'Answer keymule’s questions here'),
        h('span', null, first
          ? 'There’s no screen for keymule’s popups on this machine. Pair this browser with the code `keymule ui` printed in its terminal.'
          : 'Another browser is paired. Enter the code `keymule ui` printed in your terminal, or leave it empty to ask that browser.')),
      h('div', { class: 'pairgo' },
        input,
        h('button', { class: 'btn primary', type: 'button', disabled: ap.busy, onclick: go }, ap.busy ? 'Pairing…' : 'Pair this browser')),
      ap.err && !ap.pending.length ? h('p', { class: 'askerr' }, ap.err) : null))
  }
  if (nudge && !show) {
    parts.push(h('div', { class: 'pairing slim' },
      h('div', { class: 'pairtext' },
        h('strong', null, 'Get a notification when keymule asks'),
        h('span', null, 'Questions wait here and time out after 3 minutes.')),
      h('div', { class: 'pairgo' },
        h('button', { class: 'btn', type: 'button', onclick: () => {
          notifyLater = true
          try { localStorage.setItem('keymule-notify-later', '1') } catch {}
          renderApprovals()
        } }, 'Not now'),
        notifyButton('Turn on'))))
  }
  el.replaceChildren(...parts)
}

function renderApprovals() {
  renderPairing()
  renderAsk()
  renderTitle()
  notifyNew()
}

document.addEventListener('keydown', e => {
  if (e.key !== 'Escape' || $('ask').hidden || ap.busy || !$('reveal').hidden) return
  const item = ap.pending.find(p => !ap.hidden.has(p.req.id))
  if (!item) return
  e.preventDefault()
  if (paired()) answer(item, item.req.spec.cancel)
  else hideAsk(item)
})

watchApprovals()
document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') loadApprovals() })
