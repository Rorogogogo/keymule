// The page's state and everything that changes it. Components read the
// store and call these; nothing here touches the DOM except the title,
// the theme attribute and notifications.
import { api, browserName, b64, enc, hexOf, myDevice, myDeviceId, openShow, sealSecret, sha256 } from './api'
import { createStore } from './store'
import type {
  AuditLine, CommandReply, Device, Doctor, List, Pending, Plan, Revealed, ScanStatus, Show, StateReply, Stats, Update,
} from './types'

export type View = 'overview' | 'credentials' | 'servers' | 'leaks' | 'usage' | 'activity'
export type Theme = 'system' | 'light' | 'dark'
export type ToastKind = 'ok' | 'bad' | 'run'

const session = <T>(k: string, d: T, parse: (v: string) => T) => {
  try { const v = sessionStorage.getItem(k); return v == null ? d : parse(v) } catch { return d }
}
const pref = (k: string, d: string) => { try { return localStorage.getItem(`keymule-${k}`) || d } catch { return d } }
const setPref = (k: string, v: string) => { try { localStorage.setItem(`keymule-${k}`, v) } catch { /* private mode */ } }

export interface ConvertForm { to: 'ssh' | 'password'; host: string; username: string; port: string; description: string }

export const store = createStore({
  list: null as List | null,
  audit: [] as AuditLine[],
  version: '',
  update: null as Update | null,
  host: 'ui',
  live: null as boolean | null, // null before the first answer
  forbidden: false,
  error: '' as string | undefined,
  plan: null as Plan | null,
  planOpen: false,
  confirmApply: false,
  doctor: null as Doctor,
  doctorId: undefined as number | undefined,
  scan: null as ScanStatus | null,
  more: {} as Record<string, number>, // extra rows shown per list, 20 at a time
  menu: null as string | null, // the row whose ⋯ menu is open
  busy: [] as string[],
  filter: null as string | null,
  describing: null as string | null, // the row whose note is being edited
  converting: null as string | null, // the MCP server secret being moved to Credentials
  convertForm: null as ConvertForm | null,
  view: session<View>('keymule-view', 'overview', v => v as View),
  stats: null as Stats | null,
  urange: session('keymule-urange', 30, v => Number(v) || 30),
  utable: false,
  toast: null as { text: string; kind: ToastKind; n: number } | null,
  theme: pref('theme', 'system') as Theme,
  collapsed: pref('sidebar', 'open') === 'collapsed',
  // Questions keymule asks in the page (no screen for a popup).
  ap: {
    pending: [] as Pending[],
    devices: [] as Device[],
    pageMode: false,
    live: false,
    busy: false,
    err: '',
    value: '',
    code: '',
    version: '',
    hidden: [] as string[],
  },
  revealed: null as Revealed | null,
  notifyLater: pref('notify-later', '') === '1',
  notifyPerm: (typeof Notification !== 'undefined' ? Notification.permission : 'denied') as NotificationPermission,
})
export type AppState = ReturnType<typeof store.get>
const { get, set } = store
const setAp = (p: Partial<AppState['ap']>) => set(s => ({ ap: { ...s.ap, ...p } }))
export const isBusy = (key: string) => get().busy.includes(key)

// ---- toast ----

let toastTimer: ReturnType<typeof setTimeout> | undefined
let toastN = 0
export function toast(text: string, kind: ToastKind = 'ok', sticky = false) {
  clearTimeout(toastTimer)
  set({ toast: { text, kind, n: ++toastN } })
  if (!sticky) toastTimer = setTimeout(() => set({ toast: null }), 7000)
}

// ---- loading ----

export async function refresh() {
  try {
    const s = await api<StateReply>('/api/state')
    set({
      list: s.list, audit: s.audit || [], version: s.version, update: s.update || null,
      host: s.host || 'ui', live: true, error: s.error,
    })
  } catch (e) {
    set({ live: false, forbidden: (e as Error).message === 'forbidden' })
  }
}

export async function loadPlan() {
  try {
    const r = await api<CommandReply>('/api/plan', {})
    const text = (r.stdout || '').trim()
    const has = /secrets? in plain text:/.test(text)
    const head = text.split('\nNot adopted:')[0]
    const names = has ? [...head.matchAll(/^ {2}(\S+) {3}/gm)].map(m => m[1]) : []
    set({ plan: { text, has, names, summary: has ? text.split('\n')[0].replace(/:$/, '') : '' } })
  } catch { set({ plan: null }) }
}

export async function loadStats() {
  try {
    set({ stats: await api<Stats>(`/api/stats?days=${get().urange}&tz=${-new Date().getTimezoneOffset()}`) })
  } catch { /* keep what we have */ }
}

export function setRange(n: number) {
  set({ urange: n })
  try { sessionStorage.setItem('keymule-urange', String(n)) } catch { /* private mode */ }
  loadStats()
}

// ---- actions: each runs a keymule command, which opens keymule's own dialog ----

export async function run(key: string, start: string, call: () => Promise<CommandReply>, after?: () => unknown) {
  if (isBusy(key)) return
  set(s => ({ busy: [...s.busy, key] }))
  toast(start, 'run', true)
  try {
    const r = await call()
    const msg = (r.ok ? r.stdout : r.stderr || r.stdout || 'Something went wrong')?.trim()
    toast(msg || 'Done.', r.ok ? 'ok' : 'bad')
  } catch (e) {
    toast(`keymule ui isn't reachable: ${(e as Error).message}`, 'bad')
  } finally {
    set(s => ({ busy: s.busy.filter(b => b !== key) }))
    await refresh()
    if (after) await after()
  }
}

export const rotate = (name: string) => run(`rotate:${name}`,
  `A keymule dialog is open for "${name}". Type the new value there.`,
  () => api('/api/rotate', { name }), loadPlan)

export const remove = (name: string) => run(`delete:${name}`, `A keymule window asks before deleting "${name}".`,
  () => api('/api/delete', { name }), loadPlan)

export const reveal = (name: string) => run(`reveal:${name}`,
  get().ap.pageMode ? `keymule asks here before showing "${name}".` : `Look for the keymule window: it asks before showing "${name}".`,
  () => api('/api/reveal', { name }))

export const saveNote = (name: string, description: string) => {
  set({ describing: null })
  return run(`describe:${name}`, 'Saving…', () => api('/api/describe', { name, description: description.replace(/\s+/g, ' ').trim() }))
}

export function applyPlan() {
  if (!get().confirmApply) {
    set({ confirmApply: true })
    setTimeout(() => set({ confirmApply: false }), 4000)
    return
  }
  set({ confirmApply: false })
  run('adopt', 'Moving the keys into the keychain…', () => api('/api/adopt', {}), loadPlan)
}

// ---- rows, menus, views ----

export const isOpen = (s: AppState, name: string) => s.filter === name || s.describing === name

export function toggleRow(name: string) {
  const s = get()
  if (isOpen(s, name)) set({ filter: null, describing: s.describing === name ? null : s.describing })
  else set({ filter: name })
}
export const toggleFilter = (name: string) => set(s => ({ filter: s.filter === name ? null : name }))
export const editNote = (name: string) => set({ filter: name, describing: name, menu: null })
export const setMenu = (name: string | null) => set({ menu: name })
export const showMore = (key: string, n: number) => set(s => ({ more: { ...s.more, [key]: n } }))

export function go(view: View) {
  set({ view, more: {} })
  try { sessionStorage.setItem('keymule-view', view) } catch { /* private mode */ }
  window.scrollTo(0, 0)
}

// ---- preferences: theme and a collapsed sidebar, per browser ----

const NEXT_THEME: Record<Theme, Theme> = { system: 'light', light: 'dark', dark: 'system' }
export function nextTheme() {
  const theme = NEXT_THEME[get().theme] || 'system'
  setPref('theme', theme)
  set({ theme })
}
export function toggleSidebar() {
  const collapsed = !get().collapsed
  setPref('sidebar', collapsed ? 'collapsed' : 'open')
  set({ collapsed })
}

// ---- the leak check: it runs in keymule's background; the page polls ----

let scanTimer: ReturnType<typeof setTimeout> | undefined
function applyScan(r: ScanStatus) {
  const s = get()
  let doctor = s.doctor
  if (r.running) doctor = 'running'
  else if (r.error) doctor = { error: r.error }
  else if (r.report && r.report.entries) {
    if (s.doctor === 'running' || s.doctor === null || s.doctorId !== r.id) doctor = r.report
  }
  set({ scan: r, doctor, doctorId: r.id })
}
export async function pollScan() {
  clearTimeout(scanTimer)
  try { applyScan(await api<ScanStatus>('/api/doctor')) } catch { /* try again later */ }
  if (get().scan?.running) scanTimer = setTimeout(pollScan, 400)
}
export async function startScan() {
  if (get().doctor === 'running') return
  set(s => ({ doctor: 'running', scan: null, more: { ...s.more, leaks: 0 } }))
  try {
    applyScan(await api<ScanStatus>('/api/doctor', {}))
  } catch (e) {
    set({ doctor: { error: (e as Error).message } })
  }
  if (get().scan?.running) scanTimer = setTimeout(pollScan, 400)
}

// ---- questions keymule asks here ----
//
// A typed secret is encrypted to the asking process; a value to show is
// encrypted to a one-off key whose private half stays in this tab.

const revealKeys: Record<string, { priv: CryptoKey; asker: string }> = {}
export const paired = (s: AppState = get()) => !!myDeviceId() && s.ap.devices.some(d => d.id === myDeviceId())
export const usesPage = (s: AppState = get()) => s.ap.pageMode || s.ap.devices.length > 0
export const canNotify = () => 'Notification' in window && window.isSecureContext
export const notifyAsk = (s: AppState = get()) => canNotify() && usesPage(s) && s.notifyPerm === 'default'

// With `wait`, the server holds the request until something changes (up to
// 25 s). Chrome slows timers in background tabs to once a minute, but not
// an answer to a request, so a question still shows (and notifies) at once.
export async function loadApprovals(wait = false) {
  const s = get()
  try {
    const r = await api<{ version?: string; pending?: string[]; devices?: Device[]; page_mode?: boolean; shows?: Show[] }>(
      `/api/approvals${wait ? `?wait=${s.ap.version}` : ''}`)
    const pending: Pending[] = (r.pending || []).map(raw => ({ raw, req: JSON.parse(raw) }))
    const ids = new Set(pending.map(p => p.req.id))
    const devices = r.devices || []
    const pageMode = !!r.page_mode
    if (pageMode || devices.length) await myDevice()
    setAp({
      version: r.version || '', pending, devices, pageMode, live: true,
      hidden: get().ap.hidden.filter(id => ids.has(id)),
    })
    for (const sh of r.shows || []) {
      const k = revealKeys[sh.id]
      if (!k) continue
      delete revealKeys[sh.id]
      openShow(sh, k).then(showRevealed).catch(e => toast(`Couldn’t open the value: ${e.message}`, 'bad'))
    }
  } catch {
    setAp({ live: false })
  }
  afterApprovals()
  return get().ap.live
}

async function watchApprovals() {
  for (;;) {
    if (!await loadApprovals(true)) await new Promise(r => setTimeout(r, 2000))
  }
}

export async function pairThis(code: string) {
  setAp({ busy: true, err: '' })
  try {
    const d = await myDevice()
    const r = await api<CommandReply>('/api/pair', { name: browserName(), key: d.pub, code })
    if (!r.ok) setAp({ err: r.stderr || 'Pairing failed' })
    else { setAp({ code: '' }); toast('Paired. This browser answers keymule’s questions and opens the page without the link', 'ok') }
  } catch (e) {
    setAp({ err: (e as Error).message })
  }
  setAp({ busy: false })
  await loadApprovals()
}

export async function answer(item: Pending, button: string) {
  const { req, raw } = item
  const s = req.spec
  setAp({ busy: true, err: '' })
  try {
    const d = await myDevice()
    let secret: { epk: string; iv: string; ct: string } | null = null
    if (s.secure && button !== s.cancel) {
      const value = get().ap.value
      if (!value.trim()) { setAp({ err: 'Type the secret first', busy: false }); return }
      secret = await sealSecret(req, value)
    }
    if (s.reveal && button !== s.cancel) {
      // A one-off key for keymule to encrypt the value to. Its private half
      // stays in this tab (not extractable) and is dropped once used.
      const eph = await crypto.subtle.generateKey({ name: 'ECDH', namedCurve: 'P-256' }, false, ['deriveBits'])
      secret = { epk: b64(await crypto.subtle.exportKey('raw', eph.publicKey)), iv: '', ct: '' }
      revealKeys[req.id] = { priv: eph.privateKey, asker: req.epk }
    }
    const sec = secret ? hexOf(await sha256(`${secret.epk}.${secret.iv}.${secret.ct}`)) : ''
    const msg = `keymule-approval-v1\n${req.id}\n${hexOf(await sha256(raw))}\n${button}\n${sec}`
    const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, d.keyPair.privateKey, enc(msg))
    const r = await api<CommandReply>('/api/approvals/answer', { id: req.id, reply: { button, device: d.id, sig: b64(sig), secret } })
    if (!r.ok) setAp({ err: r.stderr || 'That didn’t go through' })
    else setAp({ value: '', pending: get().ap.pending.filter(p => p.req.id !== req.id) })
  } catch (e) {
    setAp({ err: (e as Error).message })
  }
  setAp({ busy: false })
  afterApprovals()
}

// Saying no needs no signature, so a browser that isn't paired can do it
// too and is never stuck behind a question.
export async function deny(item: Pending) {
  setAp({ busy: true, err: '' })
  try {
    const r = await api<CommandReply>('/api/approvals/deny', { id: item.req.id })
    if (!r.ok) setAp({ err: r.stderr || 'That didn’t go through' })
    else setAp({ value: '', pending: get().ap.pending.filter(p => p.req.id !== item.req.id) })
  } catch (e) {
    setAp({ err: (e as Error).message })
  }
  setAp({ busy: false })
  afterApprovals()
}

// Close the question in this browser only; it keeps waiting.
export const hideAsk = (item: Pending) => setAp({ hidden: [...get().ap.hidden, item.req.id], err: '' })
export const showHidden = () => setAp({ hidden: [] })
export const setAskValue = (value: string) => setAp({ value })
export const setPairCode = (code: string) => setAp({ code })

export function showRevealed(data: Revealed) { set({ revealed: data }) }
export const closeRevealed = () => set({ revealed: null })

// ---- notifications and the tab title ----
//
// keymule's questions wait in this page and time out after 3 minutes, so tell
// the user even when they're looking at a terminal: a system notification
// (the browser's) and a count in the tab title.

export async function askNotifications() {
  try { await Notification.requestPermission() } catch { /* not supported */ }
  set({ notifyPerm: Notification.permission })
  if (Notification.permission === 'granted') toast('You’ll get a notification when keymule asks', 'ok')
}
export function notNow() {
  setPref('notify-later', '1')
  set({ notifyLater: true })
}

const notes = new Map<string, Notification | null>()
function notifyNew() {
  const { pending } = get().ap
  const ids = new Set(pending.map(p => p.req.id))
  for (const [id, n] of notes) if (!ids.has(id)) { n?.close(); notes.delete(id) }
  if (!canNotify() || Notification.permission !== 'granted') return
  for (const p of pending) {
    if (notes.has(p.req.id)) continue
    if (document.visibilityState === 'visible' && document.hasFocus()) { notes.set(p.req.id, null); continue }
    try {
      const n = new Notification('keymule asks', { body: p.req.spec.title, tag: `keymule-${p.req.id}`, requireInteraction: true })
      n.onclick = () => { window.focus(); setAp({ hidden: get().ap.hidden.filter(id => id !== p.req.id) }); n.close() }
      notes.set(p.req.id, n)
    } catch { notes.set(p.req.id, null) }
  }
}

const baseTitle = document.title
function afterApprovals() {
  const n = get().ap.pending.length
  document.title = n ? `(${n}) keymule asks` : baseTitle
  notifyNew()
}

// ---- start: load everything, then keep it fresh ----

let started = false
export function start() {
  if (started) return
  started = true
  refresh()
  loadStats()
  loadPlan()
  pollScan()
  setInterval(refresh, 20000)
  let statsKey = ''
  setInterval(async () => {
    try {
      const r = await api<{ audit?: AuditLine[] }>('/api/audit')
      const audit = r.audit || []
      set({ audit, live: true })
      const last = audit[audit.length - 1]
      const key = `${audit.length}|${last?.ts}|${new Date().toDateString()}`
      if (key !== statsKey) { statsKey = key; loadStats() }
    } catch (e) {
      set({ live: false, forbidden: (e as Error).message === 'forbidden' })
    }
  }, 3000)
  setInterval(loadPlan, 60000)
  watchApprovals()
  document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'visible') loadApprovals() })
}
