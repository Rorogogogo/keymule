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

