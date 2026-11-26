import { store } from '../lib/app'
import type { LeakEntry, Report, ScanStatus } from '../lib/types'
import { Paged } from './rows'
import { Badge, Ico, Join, Mono } from './ui'

const PHASES: Record<string, string> = {
  transcripts: 'Reading agent transcripts', configs: 'Checking agent configs', env: 'Looking for .env files',
  history: 'Checking shell history', stored: 'Comparing with the keys in keymule', done: 'Finishing up',
}
// Transcripts are most of the work; the rest is quick.
const PHASE_AT: Record<string, number> = { configs: 0.92, env: 0.94, history: 0.96, stored: 0.98, done: 1 }
const size = (b: number) => (b >= 1 << 30 ? `${(b / (1 << 30)).toFixed(1)} GB` : `${Math.round(b / (1 << 20))} MB`)
const clock = (ms: number) => { const s = Math.floor((ms || 0) / 1000); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}` }

function ScanProgress({ r }: { r: ScanStatus | null }) {
  const frac = !r ? 0 : r.phase === 'transcripts' ? (r.total ? 0.9 * r.done / r.total : 0) : (PHASE_AT[r.phase || ''] ?? 0)
  const parts: string[] = []
  if (r?.total) parts.push(`${size(r.done)} of ${size(r.total)}`)
  if (r && r.phase === 'transcripts' && r.done > 0 && r.elapsed_ms > 2000) {
    const left = (r.total - r.done) / (r.done / r.elapsed_ms) / 1000
    parts.push(left < 5 ? 'almost done' : left < 60 ? `about ${Math.ceil(left / 5) * 5} s left` : `about ${Math.ceil(left / 60)} min left`)
  }
  if (r?.lines) parts.push(`${r.found} possible ${r.found === 1 ? 'key' : 'keys'} so far`)
  return (
    <div className="scanrun">
      <div className="scanhead">
        <Ico name="spin" />
        <span className="phase">{r ? PHASES[r.phase || ''] || 'Scanning' : 'Starting the scan…'}</span>
        <span className="time">{r ? clock(r.elapsed_ms) : ''}</span>
      </div>
      <div className="scanbar" role="progressbar" aria-label="Scan progress" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(frac * 100)}>
        <span className="fill" style={{ transform: `scaleX(${frac})` }} />
      </div>
      <div className="scanmeta">{parts.length ? parts.join(' · ') : 'Values stay masked, and nothing leaves this Mac.'}</div>
      <div className="scanlog" aria-live="polite">
        {(r?.log || []).slice(-6).map((line, i) => <div key={(r?.lines || 0) - 6 + i} className="ln">{line}</div>)}
      </div>
    </div>
  )
}

const SEVERITY = { leaked: 0, plaintext: 1, readable: 2 }
const SEV_LABEL = { leaked: 'sent to a model', plaintext: 'plain text in a config', readable: 'readable by agents' }
const worst = (e: LeakEntry) => e.locations.map(l => l.severity).sort((a, b) => SEVERITY[a] - SEVERITY[b])[0]

function LeakRow({ e }: { e: LeakEntry }) {
  const sev = worst(e)
  const first = e.locations[0] || { place: '' }
  const more = e.locations.length > 1 ? `+${e.locations.length - 1} more` : null
  return (
    <div className="row" title={e.locations.map(l => `${l.place}${l.date ? ' · ' + l.date : ''}`).join('\n')}>
      <Ico name={sev === 'leaked' ? 'warn' : 'eye'} />
      <span className="body">
        <span className="title"><Mono>{e.masked}</Mono></span>
        <span className="sub"><Join parts={[e.label, SEV_LABEL[sev], first.place, first.date, more]} /></span>
      </span>
      <span className="acc">
        {e.stored_as ? <Badge kind="ok">In keymule</Badge> : sev === 'leaked' ? <Badge kind="bad">Rotate it</Badge> : sev === 'plaintext' ? <Badge kind="warn">Move it</Badge> : <Badge>Readable</Badge>}
      </span>
    </div>
  )
}

export function Leaks() {
  const d = store.use(s => s.doctor)
  const scan = store.use(s => s.scan)
  if (d === null) {
    return <div className="scanbox"><Ico name="shield" /><span>Look for keys that leaked into agent transcripts, sit in plain text in configs, or are readable in .env files and shell history. Values stay masked.</span></div>
  }
  if (d === 'running') return <ScanProgress r={scan} />
  if ('error' in d) return <div className="scanbox"><Ico name="x" /><span className="t-bad">{d.error}</span></div>
  return <Report d={d} />
}

function Report({ d }: { d: Report }) {
  const entries = [...d.entries].sort((a, b) => SEVERITY[worst(a)] - SEVERITY[worst(b)])
  const count = (sev: keyof typeof SEVERITY) => entries.filter(e => worst(e) === sev).length
  const stored = entries.filter(e => e.stored_as).length
  const st = d.stats || {}
  const c = (k: string, v: number, cls: string) => <div key={k}><span className="k">{k}</span><span className={`v ${v ? cls : ''}`}>{v}</span></div>
  return (
    <>
      <div className="counts">
        {c('Leaked', count('leaked'), 't-bad')}{c('Plain text', count('plaintext'), 't-warn')}{c('Readable', count('readable'), '')}{c('In keymule', stored, '')}
      </div>
      {entries.length
        ? <Paged rows={entries} id="leaks" base={10} render={(e, i) => <LeakRow key={`${e.masked}${i}`} e={e} />} />
        : <div className="empty">Nothing found in {st.transcripts ?? 0} transcripts, {(st.configs || []).length} configs and {st.env_files ?? 0} .env files.</div>}
    </>
  )
}
