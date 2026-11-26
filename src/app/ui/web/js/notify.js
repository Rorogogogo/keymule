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
