// Questions keymule asks in the page, laid out like its popup, and a value
// shown after you said Show.
import { useEffect, useRef, useState } from 'react'
import {
  answer, closeRevealed, deny, hideAsk, notifyAsk, paired, pairThis, setAskValue, setPairCode, store,
} from '../lib/app'
import type { AskSpec, Pending } from '../lib/types'
import { NotifyButton } from './Banners'
import { Icon, type IconName } from './ui'

const ASK_ICON: Record<string, IconName> = { key: 'key', unlock: 'shield', trash: 'x', eye: 'eye', shield: 'shield' }

// The text: paragraphs; lines indented by 8 spaces are code, and a short
// paragraph right before code is its label.
function AskBody({ body }: { body?: string }) {
  const paras = (body || '').split(/\n\n+/).filter(p => p.trim())
  const isCode = (p?: string) => !!p && p.split('\n').every(l => l.startsWith('        '))
  return (
    <>
      {paras.map((p, i) => isCode(p)
        ? <pre key={i} className="askcode">{p.split('\n').map(l => l.slice(8)).join('\n')}</pre>
        : <p key={i} className={isCode(paras[i + 1]) && p.length < 60 ? 'asklabel' : undefined}>{p}</p>)}
    </>
  )
}

// The field for a secret: one line, or several (a private key, AWS keys),
// hidden either way, with an eye to check what was typed. Enter saves one
// line; ⌘/Ctrl-Enter saves several.
function SecretField({ item, s }: { item: Pending; s: AskSpec }) {
  const value = store.use(st => st.ap.value)
  const [peek, setPeek] = useState(false)
  useEffect(() => setPeek(false), [item.req.id]) // hidden again for each new question
  const common = {
    id: 'askval', autoComplete: 'off', spellCheck: false, value,
    onChange: (e: { target: { value: string } }) => setAskValue(e.target.value),
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key === 'Enter' && (!s.multiline || e.metaKey || e.ctrlKey)) { e.preventDefault(); answer(item, s.default) }
    },
  }
  return (
    <div className="askvalwrap">
      {s.multiline
        ? <textarea {...common} className={`field askval multi${peek ? ' shown' : ''}`} placeholder="Paste it here (⌘↩ saves)" />
        : <input {...common} className="field askval" type={peek ? 'text' : 'password'} placeholder="Paste or type it here" />}
      <button
        className="peek" type="button" title={peek ? 'Hide' : 'Show'}
        aria-label={peek ? 'Hide the value' : 'Show the value'} aria-pressed={peek}
        onMouseDown={e => e.preventDefault()} // the cursor stays in the field
        onClick={() => setPeek(!peek)}
      >
        <Icon name={peek ? 'eyeOff' : 'eye'} />
      </button>
    </div>
  )
}

// Shown in the question when this browser can't answer yet: pair right here.
function PairInCard() {
  const code = store.use(s => s.ap.code)
  const busy = store.use(s => s.ap.busy)
  const devices = store.use(s => s.ap.devices.length)
  const go = () => pairThis(code)
  return (
    <div className="askpair">
      <p>This browser isn’t paired, so it can only say no. To answer, enter the code `keymule ui` printed in your terminal.</p>
      {devices ? <p className="muted">Paired before? Use the same address: 127.0.0.1 and localhost count as different.</p> : null}
      <div className="pairgo">
        <input id="askcode" className="field paircode" inputMode="numeric" placeholder="123-456" aria-label="Pairing code"
          value={code} onChange={e => setPairCode(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); go() } }} />
        <button className="btn primary" type="button" disabled={busy} onClick={go}>{busy ? 'Pairing…' : 'Pair'}</button>
      </div>
    </div>
  )
}

export function AskCard() {
  const st = store.use()
  const { ap } = st
  const shown = ap.pending.filter(p => !ap.hidden.includes(p.req.id))
  const item = ap.live ? shown[0] : undefined
  const me = paired(st)
  const wrap = useRef<HTMLDivElement>(null)
  // Focus once per question: its field, else the safe button, never the
  // one that says yes. A question can wait in a tab for minutes, and a
  // stray Space or Return would press it.
  const id = item?.req.id
  useEffect(() => {
    if (!id || !wrap.current) return
    const field = wrap.current.querySelector<HTMLElement>('#askval, #askcode')
    if (field) field.focus()
    else wrap.current.querySelector<HTMLElement>('[data-safe]')?.focus()
  }, [id, me])
  if (!item) return null
  const s = item.req.spec
  const noLabel = s.cancel === 'Cancel' ? 'Deny' : s.cancel
  return (
    <div id="ask" className="askwrap" ref={wrap}>
      <div className={`ask ${s.tone || ''}`} role="dialog" aria-modal="true" aria-labelledby="asktitle">
        <div className="askbrand">keymule asks{shown.length > 1 ? <span className="askmore">+{shown.length - 1} more</span> : null}</div>
        <div className="askmain">
          <div className="askicon"><Icon name={ASK_ICON[s.icon || ''] || 'key'} /></div>
          <div className="asktext">
            <h2 id="asktitle">{s.title}</h2>
            <AskBody body={s.body} />
            {s.secure && me ? <SecretField item={item} s={s} /> : null}
            {me ? null : <PairInCard />}
            {ap.err ? <p className="askerr">{ap.err}</p> : null}
          </div>
        </div>
        {notifyAsk(st) ? <div className="asknotify"><NotifyButton label="Get a notification next time" /></div> : null}
        {me ? (
          <div className="askfoot">
            {s.buttons.map((b, i) => [
              <button
                key={b} type="button" disabled={ap.busy} data-safe={b === s.cancel ? '1' : undefined}
                className={`btn${b === s.destructive ? ' danger-fill' : b === s.primary ? ' primary' : ''}`}
                onClick={() => answer(item, b)}
              >{b}</button>,
              i === 0 && s.buttons.length > 2 ? <span key="grow" className="grow" /> : null,
            ])}
          </div>
        ) : (
          <div className="askfoot">
            <button className="btn" type="button" disabled={ap.busy} onClick={() => hideAsk(item)}>Hide</button>
            <span className="grow" />
            <button className="btn" type="button" disabled={ap.busy} data-safe="1" onClick={() => deny(item)}>{noLabel}</button>
          </div>
        )}
      </div>
    </div>
  )
}

// A value keymule encrypted for this tab: shown for 30 seconds.
export function RevealCard() {
  const data = store.use(s => s.revealed)
  const [left, setLeft] = useState(30)
  const [copied, setCopied] = useState('')
  const done = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!data) return
    const until = Date.now() + 30000
    setCopied('')
    const tick = () => {
      const s = Math.max(0, Math.ceil((until - Date.now()) / 1000))
      setLeft(s)
      if (!s) closeRevealed()
    }
    tick()
    done.current?.focus()
    const t = setInterval(tick, 1000)
    return () => clearInterval(t)
  }, [data])
  if (!data) return null
  return (
    <div id="reveal" className="askwrap">
      <div className="ask" role="dialog" aria-modal="true" aria-label={`${data.name}, shown for 30 seconds`}>
        <div className="askbrand">keymule · only this browser can read this</div>
        <div className="askmain">
          <span className="askicon"><Icon name="eye" /></span>
          <div className="asktext">
            <h2>{data.name}</h2>
            {(data.items || []).map(([k, v], i) => [
              <p key={`k${i}`} className="asklabel">{k}</p>,
              <pre key={`v${i}`} className="askcode revealval">{v}</pre>,
            ])}
          </div>
        </div>
        <div className="askfoot">
          <span className="muted">Closes in {left} s</span>
          <span className="grow" />
          {data.copy ? (
            <button className="btn" type="button" onClick={async () => {
              try { await navigator.clipboard.writeText(data.copy!); setCopied('Copied') } catch { setCopied('Couldn’t copy') }
            }}>{copied || 'Copy'}</button>
          ) : null}
          <button ref={done} className="btn primary" type="button" onClick={closeRevealed}>Done</button>
        </div>
      </div>
    </div>
  )
}

const TOAST_ICON: Record<string, IconName> = { ok: 'check', bad: 'x', run: 'spin' }
export function Toast() {
  const t = store.use(s => s.toast)
  if (!t) return null
  return (
    <div id="toast" role="status">
      <span className={`ico ${t.kind}${t.kind === 'run' ? ' spinning' : ''}`} aria-hidden="true"><Icon name={TOAST_ICON[t.kind] || 'check'} /></span>
      <span className="body">{t.text}</span>
    </div>
  )
}
