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
  globe: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'M2.5 12h19M12 2.5c3.2 3.4 3.2 15.6 0 19M12 2.5c-3.2 3.4-3.2 15.6 0 19' }]],
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

