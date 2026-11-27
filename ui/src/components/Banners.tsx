import { useState } from 'react'
import { api } from '../lib/api'
import {
  applyPlan, askNotifications, go, isBusy, notifyAsk, notNow, paired, pairThis, setPairCode, showHidden, store, usesPage,
} from '../lib/app'
import { KINDS, Swatch } from './Usage'
import { Ico, Join, plural } from './ui'

function CopyButton({ text }: { text: string }) {
  const [label, setLabel] = useState('Copy')
  return (
    <button className="btn" type="button" onClick={async () => {
      try { await navigator.clipboard.writeText(text); setLabel('Copied') } catch { setLabel('Couldn’t copy') }
    }}>{label}</button>
  )
}

function RestartButton() {
  const [busy, setBusy] = useState(false)
  return (
    <button className="btn primary" type="button" disabled={busy} onClick={async () => {
      setBusy(true)
      try { await api('/api/restart', {}) } catch { /* it's restarting */ }
    }}>{busy ? 'Restarting…' : 'Restart now'}</button>
  )
}

// A newer release, or an upgrade this page's keymule hasn't restarted onto.
// Quiet on purpose: everything still works. keymule never updates itself.
export function UpdateBanner() {
  const u = store.use(s => s.update)
  const host = store.use(s => s.host)
  if (!u) return null
  if (u.replaced) {
    const menubar = host === 'menubar'
    return (
      <div className="pairing slim update">
        <div className="pairtext">
          <strong>keymule was updated. Restart it to use the new version</strong>
          <span>{menubar
            ? 'Restart your agents too (Claude Code, Codex, Cursor), so the keymule they started is the new one.'
            : 'Stop `keymule ui` and start it again. Restart your agents too, so the keymule they started is the new one.'}</span>
        </div>
        {menubar ? <div className="pairgo"><RestartButton /></div> : null}
      </div>
    )
  }
  if (!u.latest) return null
  return (
    <div className="pairing slim update">
      <div className="pairtext">
        <strong>keymule {u.latest} is out</strong>
        <span>You have {u.current}. <a href={u.notes} target="_blank" rel="noopener">What’s new</a>. To update, run:</span>
        <code className="updcmd">{u.command}</code>
      </div>
      <div className="pairgo"><CopyButton text={u.command || ''} /></div>
    </div>
  )
}

// No keychain and no passphrase, or keys in plain text that keymule can move.
export function Callout() {
  const vp = store.use(s => s.list?.vault_problem)
  const p = store.use(s => s.plan)
  const busy = store.use(() => isBusy('adopt'))
  const confirm = store.use(s => s.confirmApply)
  const planOpen = store.use(s => s.planOpen)
  if (vp) {
    // Nothing can be saved, so say how to fix it first.
    const [why, ...help] = vp.split('\n\n')
    return (
      <div className="callout">
        <Ico name="warn" />
        <div className="body">
          <span className="title">keymule can’t store secrets on this machine yet</span>
          <span className="sub">{why}. Set a passphrase where your agents run, then restart them:</span>
          <pre className="plan">{help.join('\n\n')}</pre>
        </div>
      </div>
    )
  }
  if (!p || !p.has) return null
  return (
    <div className="callout">
      <Ico name={busy ? 'spin' : 'warn'} />
      <div className="body">
        <span className="title">{p.summary}</span>
        <span className="sub">{p.names.join(', ')}: keymule can move them into the keychain, and the servers keep working.</span>
        <div className="acts">
          <button className="btn primary" type="button" disabled={busy} onClick={applyPlan}>{confirm ? 'Click again to confirm' : 'Move to keychain'}</button>
          <button className="btn" type="button" onClick={() => store.set({ planOpen: !planOpen })}>{planOpen ? 'Hide details' : 'Show details'}</button>
        </div>
        {planOpen ? <pre className="plan">{p.text}</pre> : null}
      </div>
    </div>
  )
}

export function NotifyButton({ label }: { label: string }) {
  return <button className="btn link" type="button" onClick={askNotifications}>{label}</button>
}

// Pairing this browser, questions you hid, and turning on notifications.
export function PairingBanners() {
  const st = store.use()
  const ap = st.ap
  const show = ap.live && usesPage(st) && !paired(st)
  const first = ap.devices.length === 0
  const waiting = ap.pending.filter(p => ap.hidden.includes(p.req.id)).length
  const nudge = ap.live && notifyAsk(st) && !st.notifyLater
  const [code, setCode] = useState('')
  const pair = () => { setPairCode(code); pairThis(code) }
  return (
    <>
      {waiting ? (
        <div className="pairing slim">
          <div className="pairtext"><strong>{waiting === 1 ? 'A question is waiting for you' : `${waiting} questions are waiting for you`}</strong></div>
          <div className="pairgo"><button className="btn primary" type="button" onClick={showHidden}>Show</button></div>
        </div>
      ) : null}
      {show ? (
        <div className="pairing">
          <div className="pairtext">
            <strong>Answer keymule’s questions here</strong>
            <span>{first
              ? 'There’s no screen for keymule’s popups on this machine. Pair this browser with the code `keymule ui` printed in its terminal.'
              : 'Another browser is paired. Enter the code `keymule ui` printed in your terminal, or leave it empty to ask that browser.'}</span>
          </div>
          <div className="pairgo">
            <input id="paircode" className="field paircode" inputMode="numeric" placeholder="123-456" aria-label="Pairing code"
              value={code} onChange={e => setCode(e.target.value)} onKeyDown={e => { if (e.key === 'Enter') pair() }} />
            <button className="btn primary" type="button" disabled={ap.busy} onClick={pair}>{ap.busy ? 'Pairing…' : 'Pair this browser'}</button>
          </div>
          {ap.err && !ap.pending.length ? <p className="askerr">{ap.err}</p> : null}
        </div>
      ) : null}
      {nudge && !show ? (
        <div className="pairing slim">
          <div className="pairtext">
            <strong>Get a notification when keymule asks</strong>
            <span>Questions wait here and time out after 3 minutes.</span>
          </div>
          <div className="pairgo">
            <button className="btn" type="button" onClick={notNow}>Not now</button>
            <NotifyButton label="Turn on" />
          </div>
        </div>
      ) : null}
    </>
  )
}

export function StatTiles() {
  const list = store.use(s => s.list)
  const stats = store.use(s => s.stats)
  if (!list) return null
  const t = stats?.daily?.at(-1)
  const n = list.credentials.length + list.servers.length
  const parts = t ? KINDS.filter(([k]) => t[k]).map(([k, one, many]) => <span className="part"><Swatch k={k} />{plural(t[k], one, many)}</span>) : []
  return (
    <>
      <div className="stat"><span className="k">Secrets stored</span><span className="v ">{n}</span></div>
      <div className="stat">
        <span className="k">Uses today</span>
        <span className="v ">{t ? t.total.toLocaleString() : '—'}</span>
        <span className="statsub">
          {parts.length ? <Join parts={parts} /> : 'Nothing yet today'}
          <button className="btn link" type="button" onClick={() => go('usage')}>Usage</button>
        </span>
      </div>
      <div className="stat"><span className="k">Failed today</span><span className={`v ${t?.failed ? 'bad' : ''}`}>{t ? t.failed : 0}</span></div>
    </>
  )
}
