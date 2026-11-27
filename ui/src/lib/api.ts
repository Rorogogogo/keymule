// Talking to `keymule ui`: the token, signing in with a paired browser, and
// the crypto a paired browser uses (its key pair never leaves this browser).

// The token is in the URL fragment keymule opened. Keep it for this tab only
// and take it out of the address bar. Without one, a paired browser logs in
// with its pairing key (see login below).
let token: string | null = (() => {
  const m = location.hash.match(/t=([0-9a-f]{32})/)
  if (m) {
    try { sessionStorage.setItem('keymule-token', m[1]) } catch { /* private mode */ }
    history.replaceState(null, '', location.pathname)
    return m[1]
  }
  try { return sessionStorage.getItem('keymule-token') } catch { return null }
})()

export async function api<T = any>(path: string, body?: unknown, retried = false): Promise<T> {
  if (!token) await tryLogin()
  const used = token
  const r = await fetch(path, {
    method: body === undefined ? 'GET' : 'POST',
    headers: { 'X-Keymule-Token': used || '', 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  })
  if (r.status === 403) {
    // `keymule ui` restarted with a new token: log in again, once.
    if (!retried && (token !== used || await tryLogin()) && token !== used) return api<T>(path, body, true)
    throw new Error('forbidden')
  }
  return r.json()
}

// A browser paired for keymule's questions opens the page without the link:
// it signs a one-off challenge with its pairing key (which can't be read out
// of the browser) and gets the token back. Nothing to log in with until the
// browser is paired.
async function login(): Promise<boolean> {
  const d = await idb<StoredDevice | undefined>('readonly', s => s.get('me')).catch(() => undefined)
  if (!d) return false
  const post = (path: string, body: unknown) => fetch(path, {
    method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body),
  }).then(r => r.json())
  const c = await post('/login/challenge', {})
  if (!c.ok) return false
  const sig = await crypto.subtle.sign({ name: 'ECDSA', hash: 'SHA-256' }, d.keyPair.privateKey, enc(`keymule-login-v1\n${c.nonce}`))
  const r = await post('/login', { device: d.id, nonce: c.nonce, sig: b64(sig) })
  if (!r.ok) return false
  token = r.token
  try { sessionStorage.setItem('keymule-token', r.token) } catch { /* private mode */ }
  return true
}

// One login at a time: a good one is reused for 2 s, and after a failed one
// keymule waits 10 s before trying again.
let loginTry: Promise<boolean> | null = null
function tryLogin() {
  if (!loginTry) {
    const t = login().catch(() => false)
    loginTry = t
    t.then(ok => setTimeout(() => { loginTry = null }, ok ? 2000 : 10000))
  }
  return loginTry
}

// ---- crypto helpers ----

export const enc = (s: string) => new TextEncoder().encode(s)
export const b64 = (buf: ArrayBuffer | Uint8Array) => btoa(String.fromCharCode(...new Uint8Array(buf)))
export const unb64 = (s: string) => Uint8Array.from(atob(s), c => c.charCodeAt(0))
export const hexOf = (buf: ArrayBuffer) => [...new Uint8Array(buf)].map(b => b.toString(16).padStart(2, '0')).join('')
export const sha256 = (data: string | BufferSource) => crypto.subtle.digest('SHA-256', typeof data === 'string' ? enc(data) : data)

// ---- this browser's pairing key, in IndexedDB ----

export interface StoredDevice { keyPair: CryptoKeyPair; pub: string; id: string }

export function idb<T>(mode: IDBTransactionMode, fn: (s: IDBObjectStore) => IDBRequest): Promise<T> {
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

// Each browser makes its own key pair (the private half can't be read out,
// even by this page) and keeps it in IndexedDB. Pairing hands keymule the
// public half; every answer is signed with the private one, so nothing else
// can answer for you.
let me: StoredDevice | null = null
export const myDeviceId = () => me?.id ?? null
export async function myDevice(): Promise<StoredDevice> {
  if (me) return me
  let d = await idb<StoredDevice | undefined>('readonly', s => s.get('me')).catch(() => undefined)
  if (!d) {
    const keyPair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify'])
    const raw = await crypto.subtle.exportKey('raw', keyPair.publicKey)
    d = { keyPair, pub: b64(raw), id: hexOf((await sha256(raw)).slice(0, 8)) }
    const stored = d
    await idb('readwrite', s => s.put(stored, 'me'))
  }
  me = d
  return d
}

export function browserName() {
  const ua = navigator.userAgent
  const b = /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : 'Browser'
  const os = /Mac OS X/.test(ua) ? 'Mac' : /Windows/.test(ua) ? 'Windows' : /Linux/.test(ua) ? 'Linux' : ''
  return os ? `${b} on ${os}` : b
}

// A typed secret, encrypted to the keymule process that asked for it.
export async function sealSecret(req: { id: string; epk: string }, value: string) {
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
export async function openShow(sh: { id: string; iv: string; ct: string }, k: { priv: CryptoKey; asker: string }) {
  const asker = await crypto.subtle.importKey('raw', unb64(k.asker), { name: 'ECDH', namedCurve: 'P-256' }, false, [])
  const bits = new Uint8Array(await crypto.subtle.deriveBits({ name: 'ECDH', public: asker }, k.priv, 256))
  const prefix = enc(`keymule-show-v1\n${sh.id}\n`)
  const both = new Uint8Array(prefix.length + bits.length)
  both.set(prefix); both.set(bits, prefix.length)
  const aes = await crypto.subtle.importKey('raw', await sha256(both), 'AES-GCM', false, ['decrypt'])
  const plain = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: unb64(sh.iv) }, aes, unb64(sh.ct))
  return JSON.parse(new TextDecoder().decode(plain))
}
