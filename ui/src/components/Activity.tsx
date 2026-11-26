import { store, toggleFilter } from '../lib/app'
import { Paged } from './rows'
import { Badge, when } from './ui'

// Audit lines that aren't HTTP requests: what to call them.
const EVENTS: Record<string, string> = {
  START: 'Started', EXEC: 'Ran', INSTALL: 'Copied to git', UNINSTALL: 'Removed from git',
  'GH-SECRET': 'Copied to GitHub', 'AUTO-ALLOW': 'Allowed', 'ALLOW-ALL-ON': 'Allow all on',
  'ALLOW-ALL-OFF': 'Allow all off', PAIR: 'Paired', 'BAD-APPROVAL': 'Refused',
  'PAGE-LOGIN': 'Signed in', 'ANSWERED-IN-PAGE': 'Answered', 'DENIED-IN-PAGE': 'Denied',
  SSH: 'Logged in', CONVERT: 'Moved', RENAME: 'Renamed',
}
// keymule's own events have no credential: show them as keymule's.
const actName = (name: string) => (name === '-' ? 'keymule' : name)

export function FilterChip() {
  const f = store.use(s => s.filter)
  return f ? <button className="chip" type="button" onClick={() => toggleFilter(f)} title="Show everything">{f} ×</button> : null
}

export function Activity() {
  const audit = store.use(s => s.audit)
  const f = store.use(s => s.filter)
  const all = store.use(s => s.view) === 'activity'
  const rows = audit.filter(a => !f || a.name === f).slice(-1000).reverse()
  if (!rows.length) return <div className="empty">{f ? `Nothing from ${f} yet.` : 'Requests and server starts show up here as they happen.'}</div>
  return (
    <Paged rows={rows} id="act" base={all ? 50 : 8} render={(a, i) => {
      const event = EVENTS[a.method]
      const shown = a.method === 'REVEAL'
      const ok = a.status >= 200 && a.status < 300
      return (
        <div key={`${a.ts}-${i}`} className="act">
          <span className="name" onClick={() => toggleFilter(a.name)} title="Show only its activity">{actName(a.name)}</span>
          <span className="req" title={a.url}><span className="m">{a.method}</span><span className="u">{shown ? 'shown to you in a keymule window' : a.url}</span></span>
          <span className="st">
            {event ? <Badge kind={a.method === 'BAD-APPROVAL' ? 'bad' : ''}>{event}</Badge> : shown ? <Badge kind="warn">Revealed</Badge> : <Badge kind={ok ? 'ok' : 'bad'}>{a.status || '—'}</Badge>}
          </span>
          <span className="when">{when(a.ts)}</span>
        </div>
      )
    }} />
  )
}
