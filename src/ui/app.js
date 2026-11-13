// keymule ui. Everything shown here comes from keymule commands that never
// print secrets; buttons run commands that open keymule's own dialogs.
'use strict'

const $ = id => document.getElementById(id)

// The token is in the URL fragment keymule opened. Keep it for this tab only
// and take it out of the address bar.
const token = (() => {
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
  showAllLeaks: false,
  showAllAct: false,
  busy: new Set(),
  filter: null,
  view: (() => { try { return sessionStorage.getItem('keymule-view') || 'overview' } catch { return 'overview' } })(),
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
  check: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'm8 12.5 2.5 2.5L16 9.5' }]],
  x: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'm15 9-6 6M9 9l6 6' }]],
  shield: [['path', { d: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z' }], ['path', { d: 'm9 12 2 2 4-4' }]],
  spin: [['path', { d: 'M21 12a9 9 0 1 1-6.2-8.56' }]],
  home: [['rect', { x: 3, y: 3, width: 7, height: 9, rx: 1.5 }], ['rect', { x: 14, y: 3, width: 7, height: 5, rx: 1.5 }], ['rect', { x: 14, y: 12, width: 7, height: 9, rx: 1.5 }], ['rect', { x: 3, y: 16, width: 7, height: 5, rx: 1.5 }]],
  pulse: [['path', { d: 'M3 12h4l3-8 4 16 3-8h4' }]],
  sidebar: [['rect', { x: 3, y: 4, width: 18, height: 16, rx: 2.5 }], ['path', { d: 'M9 4v16' }]],
  sun: [['circle', { cx: 12, cy: 12, r: 4 }], ['path', { d: 'M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4' }]],
  moon: [['path', { d: 'M20.5 14.1A8.5 8.5 0 1 1 9.9 3.5a6.6 6.6 0 0 0 10.6 10.6z' }]],
  monitor: [['rect', { x: 2.5, y: 4, width: 19, height: 13, rx: 2 }], ['path', { d: 'M8 21h8M12 17v4' }]],
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
const ico = (name, cls) => h('span', { class: `ico ${cls || ''}`, 'aria-hidden': 'true' }, icon(name))
const badge = (text, kind) => h('span', { class: `badge ${kind || ''}` }, text)
const join = parts => parts.filter(Boolean).flatMap((p, i) => (i ? [' · ', p] : [p]))
const mono = text => h('span', { class: 'mono' }, text)

async function api(path, body) {
  const r = await fetch(path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { 'X-Keymule-Token': token || '', 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (r.status === 403) throw new Error('forbidden')
  return r.json()
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
const setHint = () => { $('url').placeholder = URL_HINT[$('svc').value] || '' }
$('svc').addEventListener('change', setHint)
setHint()

$('scan').addEventListener('click', async () => {
  if (state.doctor === 'running') return
  state.doctor = 'running'
  render()
  try {
    const r = await api('/api/doctor', {})
    state.doctor = r.report || { error: r.stderr || 'The scan failed.' }
  } catch (e) {
    state.doctor = { error: e.message }
  }
  render()
})

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
  ['overview', 'Overview', 'home', 'Your agents use these keys without ever seeing them. Everything stays on this Mac.'],
  ['credentials', 'Credentials', 'key', 'API tokens your agents use through keymule. You type each one into a keymule dialog.'],
  ['servers', 'MCP servers', 'server', 'Secrets keymule hands to your MCP servers when they start, instead of keeping them in config files.'],
  ['leaks', 'Leak check', 'shield', 'Keys that already reached a model, sit in plain text, or are readable by agents.'],
  ['activity', 'Activity', 'pulse', 'Every request and server start that used a key, newest first.'],
]
function go(view) {
  state.view = view
  try { sessionStorage.setItem('keymule-view', view) } catch {}
  window.scrollTo(0, 0)
  render()
}
$('allact').addEventListener('click', () => go('activity'))

// ---- drawing ----------------------------------------------------------------

function rotateButton(name) {
  const busy = state.busy.has(`rotate:${name}`)
  return h('button', {
    class: 'btn', type: 'button', disabled: busy,
    onclick: e => { e.stopPropagation(); rotate(name) },
  }, busy ? 'Waiting…' : 'Rotate')
}

function rowAttrs(name) {
  return {
    class: `row click${state.filter === name ? ' sel' : ''}`,
    tabindex: '0',
    title: 'Show only its activity',
    onclick: () => toggleFilter(name),
    onkeydown: e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); toggleFilter(name) } },
  }
}

function renderCreds(creds) {
  $('ncreds').textContent = creds.length || ''
  if (!creds.length) return [h('div', { class: 'empty' }, 'No API credentials yet. Add one below, or ask your agent: it opens the same dialog.')]
  return creds.map(c => {
    const status = c.missing_secret ? badge('Missing', 'bad') : c.file_edited ? badge('Edited outside keymule', 'warn') : badge('Active', 'ok')
    return h('div', rowAttrs(c.name),
      ico('key'),
      h('span', { class: 'body' },
        h('span', { class: 'title' }, c.name),
        h('span', { class: 'sub' }, join([c.service, (c.hosts || []).join(', '), c.auth]))),
      h('span', { class: 'acc' }, status, rotateButton(c.name)))
  })
}

function renderServers(servers) {
  $('nservers').textContent = servers.length || ''
  if (!servers.length) return [h('div', { class: 'empty' }, 'No MCP server secrets yet. Plain-text keys in your agent configs show up above, ready to move.')]
  return servers.map(s => {
    const what = [...(s.env || []), ...(s.headers || []).map(x => `${x} header`)]
    if (s.arg_secrets) what.push(`${s.arg_secrets} in arguments`)
    const used = s.used_by.length
    return h('div', rowAttrs(s.name),
      ico('server'),
      h('span', { class: 'body' },
        h('span', { class: 'title' }, s.name),
        h('span', { class: 'sub' }, join([mono(what.join(', ')), used ? `used by ${used} config entr${used === 1 ? 'y' : 'ies'}` : null]))),
      h('span', { class: 'acc' }, used ? badge('Active', 'ok') : badge('Unused'), rotateButton(s.name)))
  })
}

function renderStats() {
  const list = state.list
  if (!list) return []
  const today = new Date().toDateString()
  const reqs = state.audit.filter(a => a.method !== 'START' && new Date(a.ts * 1000).toDateString() === today)
  const failed = reqs.filter(a => !(a.status >= 200 && a.status < 300)).length
  const n = list.credentials.length + list.servers.length
  const stat = (k, v, cls) => h('div', { class: 'stat' }, h('span', { class: 'k' }, k), h('span', { class: `v ${cls || ''}` }, v))
  return [stat('Secrets in keychain', n), stat('Requests today', reqs.length), stat('Failed today', failed, failed ? 'bad' : '')]
}

function renderBanner() {
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
  if (d === 'running') return [h('div', { class: 'scanbox' }, ico('spin'), h('span', {}, 'Reading transcripts, configs, .env files and shell history…'))]
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
  const shown = state.showAllLeaks ? entries : entries.slice(0, 10)
  const rows = shown.map(e => {
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
  if (entries.length > shown.length) rows.push(h('button', { class: 'more', type: 'button', onclick: () => { state.showAllLeaks = true; render() } }, `Show all ${entries.length}`))
  return [counts, ...rows]
}

function renderActivity() {
  const f = state.filter
  $('filter').replaceChildren(f ? h('button', { class: 'chip', type: 'button', onclick: () => toggleFilter(f), title: 'Show everything' }, f, ' ×') : '')
  const all = state.audit.filter(a => !f || a.name === f).slice(-150).reverse()
  if (!all.length) return [h('div', { class: 'empty' }, f ? `Nothing from ${f} yet.` : 'Requests and server starts show up here as they happen.')]
  const rows = (state.showAllAct || state.view === 'activity' ? all : all.slice(0, 8)).map(a => {
    const start = a.method === 'START'
    const ok = a.status >= 200 && a.status < 300
    return h('div', { class: 'act' },
      h('span', { class: 'name', onclick: () => toggleFilter(a.name), title: 'Show only its activity' }, a.name),
      h('span', { class: 'req', title: a.url }, h('span', { class: 'm' }, start ? 'START' : a.method), h('span', { class: 'u' }, a.url)),
      h('span', { class: 'st' }, start ? badge('Started') : badge(a.status || '—', ok ? 'ok' : 'bad')),
      h('span', { class: 'when' }, when(a.ts)))
  })
  if (all.length > rows.length) rows.push(h('button', { class: 'more', type: 'button', onclick: () => { state.showAllAct = true; render() } }, `Show ${all.length - rows.length} more`))
  return rows
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

function render() {
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
  if (list) {
    $('creds').replaceChildren(...renderCreds(list.credentials))
    $('servers').replaceChildren(...renderServers(list.servers))
  } else if (state.error) {
    $('creds').replaceChildren(h('div', { class: 'empty t-bad' }, state.error))
  }
  $('add').querySelector('button').disabled = state.busy.has('add')
  $('leaks').replaceChildren(...renderLeaks())
  $('activity').replaceChildren(...renderActivity())
}

render()
refresh()
loadPlan()
setInterval(refresh, 20000)
setInterval(async () => {
  try {
    const r = await api('/api/audit')
    state.audit = r.audit || []
    state.live = true
  } catch (e) {
    state.live = false
    state.forbidden = e.message === 'forbidden'
  }
  render()
}, 3000)
setInterval(loadPlan, 60000)
