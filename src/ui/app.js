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
  plan: null, // { text, names, count }
  planOpen: false,
  confirmApply: false,
  doctor: null, // null, 'running', or the report
  showAllLeaks: false,
  busy: new Set(),
  filter: null,
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

const mark = kind => h('span', { class: `mark ${kind || ''}`, 'aria-hidden': 'true' })
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

let toastTimer
function toast(text, kind, sticky) {
  const el = $('toast')
  clearTimeout(toastTimer)
  el.replaceChildren(mark(kind), h('span', { class: 'body' }, text))
  el.hidden = false
  if (!sticky) toastTimer = setTimeout(() => { el.hidden = true }, 7000)
}

function when(ts) {
  const d = new Date(ts * 1000)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false })
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
  $('ncreds').textContent = creds.length ? ` ${creds.length}` : ''
  if (!creds.length) return [h('div', { class: 'empty' }, 'No API credentials yet. Add one below, or ask your agent: it opens the same dialog.')]
  return creds.map(c => {
    const kind = c.missing_secret ? 'bad' : c.file_edited ? 'warn' : 'ok'
    const note = c.missing_secret ? h('span', { class: 't-bad' }, 'secret missing from the keychain')
      : c.file_edited ? h('span', { class: 't-warn' }, 'credentials.toml edited outside keymule') : null
    return h('div', rowAttrs(c.name),
      mark(kind),
      h('span', { class: 'body' },
        h('span', { class: 'title' }, c.name),
        h('span', { class: 'sub' }, join([c.service, mono((c.hosts || []).join(', ')), c.auth, note]))),
      h('span', { class: 'acc' }, rotateButton(c.name)))
  })
}

function renderServers(servers) {
  $('nservers').textContent = servers.length ? ` ${servers.length}` : ''
  if (!servers.length) return [h('div', { class: 'empty' }, 'No MCP server secrets yet. Plain-text keys in your agent configs show up above, ready to move.')]
  return servers.map(s => {
    const what = [...(s.env || []), ...(s.headers || []).map(x => `${x} header`)]
    if (s.arg_secrets) what.push(`${s.arg_secrets} in arguments`)
    const used = s.used_by.length
    return h('div', rowAttrs(s.name),
      mark(used ? 'ok' : ''),
      h('span', { class: 'body' },
        h('span', { class: 'title' }, s.name),
        h('span', { class: 'sub' }, join([mono(what.join(', ')), used ? `used by ${used} config entr${used === 1 ? 'y' : 'ies'}` : 'not in any config yet']))),
      h('span', { class: 'acc' }, rotateButton(s.name)))
  })
}

function renderBanner() {
  const p = state.plan
  if (!p || !p.has) return []
  const busy = state.busy.has('adopt')
  const out = [h('div', { class: 'row' },
    mark(busy ? 'run' : 'warn'),
    h('span', { class: 'body' },
      h('span', { class: 'title' }, p.summary),
      h('span', { class: 'sub' }, join([mono(p.names.join(', ')), 'keymule can move them into the keychain; the servers keep working']))),
    h('span', { class: 'acc' },
      h('button', { class: 'btn ghost', type: 'button', onclick: () => { state.planOpen = !state.planOpen; render() } }, state.planOpen ? 'Hide' : 'Preview'),
      h('button', { class: 'btn primary', type: 'button', disabled: busy, onclick: applyPlan }, state.confirmApply ? 'Click again to apply' : 'Apply')))]
  if (state.planOpen) out.push(h('pre', { class: 'plan' }, p.text))
  return out
}

const SEVERITY = { leaked: 0, plaintext: 1, readable: 2 }
const SEV_LABEL = { leaked: 'sent to a model', plaintext: 'plain text in a config', readable: 'readable by agents' }

function renderLeaks() {
  const d = state.doctor
  $('scan').textContent = d === 'running' ? 'Scanning…' : d ? 'Scan again' : 'Scan'
  $('scan').disabled = d === 'running'
  if (d === null) return [h('div', { class: 'empty' }, 'Look for keys that already leaked into agent transcripts, sit in plain text in agent configs, or are readable in .env files and shell history. Values are masked.')]
  if (d === 'running') return [h('div', { class: 'row' }, mark('run'), h('span', { class: 'body' }, h('span', { class: 'sub' }, 'Reading transcripts, configs, .env files and shell history…')))]
  if (d.error) return [h('div', { class: 'row' }, mark('bad'), h('span', { class: 'body' }, h('span', { class: 'sub' }, d.error)))]

  const worst = e => e.locations.map(l => l.severity).sort((a, b) => SEVERITY[a] - SEVERITY[b])[0]
  const entries = [...d.entries].sort((a, b) => SEVERITY[worst(a)] - SEVERITY[worst(b)])
  const count = sev => entries.filter(e => worst(e) === sev).length
  const stored = entries.filter(e => e.stored_as).length
  const st = d.stats || {}
  const counters = h('div', { class: 'counters' },
    [['leaked', 'Leaked', 't-bad'], ['plaintext', 'Plain text', 't-warn'], ['readable', 'Readable', '']].map(([k, label, cls]) =>
      h('div', {}, h('span', { class: `num ${count(k) ? cls : ''}` }, count(k)), h('span', { class: 'label' }, label))),
    h('div', {}, h('span', { class: 'num' }, stored), h('span', { class: 'label' }, 'In keymule')))
  if (!entries.length) {
    return [counters, h('div', { class: 'row' }, mark('ok'), h('span', { class: 'body' },
      h('span', { class: 'title' }, 'Nothing found'),
      h('span', { class: 'sub' }, `${st.transcripts ?? 0} transcripts, ${(st.configs || []).length} configs, ${st.env_files ?? 0} .env files checked`)))]
  }
  const shown = state.showAllLeaks ? entries : entries.slice(0, 25)
  const rows = shown.map(e => {
    const sev = worst(e)
    const first = e.locations[0] || {}
    const more = e.locations.length > 1 ? `+${e.locations.length - 1} more` : null
    const acc = e.stored_as ? h('span', {}, `in keymule as `, mono(e.stored_as))
      : sev === 'leaked' ? h('span', { class: 't-bad' }, 'rotate it') : null
    return h('div', { class: 'row', title: e.locations.map(l => `${l.place}${l.date ? ' · ' + l.date : ''}`).join('\n') },
      mark(sev === 'leaked' ? 'bad' : sev === 'plaintext' ? 'warn' : ''),
      h('span', { class: 'body' },
        h('span', { class: 'title' }, mono(e.masked)),
        h('span', { class: 'sub' }, join([e.label, SEV_LABEL[sev], first.place, first.date, more]))),
      h('span', { class: 'acc' }, acc))
  })
  if (entries.length > shown.length) {
    rows.push(h('button', { class: 'row click', type: 'button', onclick: () => { state.showAllLeaks = true; render() } },
      mark(), h('span', { class: 'body' }, h('span', { class: 'sub' }, `Show all ${entries.length}`))))
  }
  return [counters, ...rows]
}

function renderActivity() {
  const f = state.filter
  $('filter').replaceChildren(f ? h('button', { class: 'chip', type: 'button', onclick: () => toggleFilter(f), title: 'Show everything' }, f, ' ×') : '')
  const rows = state.audit.filter(a => !f || a.name === f).slice(-150).reverse()
  if (!rows.length) return [h('div', { class: 'empty' }, f ? `Nothing from ${f} yet.` : 'Requests and server starts show up here as they happen.')]
  return rows.map(a => {
    const start = a.method === 'START'
    const cls = start ? '' : a.status >= 200 && a.status < 300 ? 't-ok' : 't-bad'
    return h('div', { class: 'act' },
      h('span', { class: 'when' }, when(a.ts)),
      h('span', { class: 'name mono', onclick: () => toggleFilter(a.name) }, a.name),
      h('span', { class: 'what' }, start ? 'started' : a.method),
      h('span', { class: 'mono what', title: a.url }, a.url),
      h('span', { class: `code ${cls}` }, start ? '' : a.status || '—'))
  })
}

function render() {
  const live = $('live')
  if (state.live === false) {
    live.replaceChildren(h('span', { class: 't-bad' }, state.forbidden
      ? 'This page has no access: open the link `keymule ui` printed.'
      : 'Disconnected: run `keymule ui` again.'))
  } else if (state.live) {
    live.replaceChildren(h('span', { class: 't-ok' }, '●'), ' live')
  }
  const list = state.list
  const n = list ? list.credentials.length + list.servers.length : 0
  $('meta').replaceChildren(...join([
    state.version && mono(`v${state.version}`),
    list && `${n} secret${n === 1 ? '' : 's'} in the keychain`,
    'only this machine can open this page',
  ]))
  $('banner').replaceChildren(...renderBanner())
  if (list) {
    $('creds').replaceChildren(...renderCreds(list.credentials))
    $('servers').replaceChildren(...renderServers(list.servers))
  } else if (state.error) {
    $('creds').replaceChildren(h('div', { class: 'row' }, mark('bad'), h('span', { class: 'body' }, h('span', { class: 'sub' }, state.error))))
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
