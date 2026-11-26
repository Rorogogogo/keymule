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

