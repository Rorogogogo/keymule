import { useLayoutEffect, useRef, useState } from 'react'
import { setRange, store } from '../lib/app'
import type { Stats, UseKind } from '../lib/types'
import { Join, plural } from './ui'

// The kinds of use, in chart order (bottom to top). Colors: --s-<kind>.
export const KINDS: [UseKind, string, string][] = [
  ['api', 'API call', 'API calls'],
  ['command', 'command', 'commands'],
  ['mcp', 'MCP server start', 'MCP server starts'],
  ['reveal', 'reveal', 'reveals'],
]
const KIND_TITLE: Record<UseKind, string> = { api: 'API calls', command: 'Commands', mcp: 'MCP servers', reveal: 'Reveals' }
export const Swatch = ({ k }: { k: UseKind }) => <i className={`sw s-${k}`} aria-hidden="true" />

// Day numbers count days since 1970 in the viewer's time zone.
const dayDate = (d: number, opts: Intl.DateTimeFormatOptions) => new Date(d * 86400000).toLocaleDateString([], { timeZone: 'UTC', ...opts })
const dayLabel = (d: number, today: number) => d === today ? 'Today' : d === today - 1 ? 'Yesterday' : dayDate(d, { weekday: 'short', month: 'short', day: 'numeric' })

// A round top for the axis whose half is a whole number too.
function niceMax(v: number) {
  if (v <= 4) return 4
  const p = 10 ** Math.floor(Math.log10(v))
  return [1, 2, 3, 4, 5, 6, 8, 10].map(m => m * p).find(m => m >= v && m % 2 === 0)!
}

// A bar with rounded top corners, square at the baseline.
function topRounded(x: number, y: number, w: number, hgt: number, r: number) {
  r = Math.min(r, w / 2, hgt)
  return `M${x},${y + hgt}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + hgt}Z`
}

function Chart({ s, width }: { s: Stats; width: number }) {
  // Drawn at the real width, so text and bars stay their true size.
  const W = Math.max(300, width || 720), H = 220, L = 34, R = 6, T = 10, B = 24
  const pw = W - L - R, ph = H - T - B
  const days = s.daily
  const max = niceMax(Math.max(1, ...days.map(d => d.total)))
  const slot = pw / days.length
  const bw = Math.max(2, Math.min(28, slot * 0.64))
  const y = (v: number) => T + ph - (v / max) * ph
  const [hover, setHover] = useState<number | null>(null)
  const wrap = useRef<HTMLDivElement>(null)
  const tip = useRef<HTMLDivElement>(null)
  const [tipLeft, setTipLeft] = useState(0)
  // Beside the bar: to its right, or to its left near the right edge.
  useLayoutEffect(() => {
    if (hover == null || !wrap.current || !tip.current) return
    const cw = wrap.current.clientWidth
    const px = ((L + (hover + 0.5) * slot) / W) * cw
    const right = px + slot / 2 + 10
    setTipLeft(right + tip.current.offsetWidth > cw ? px - slot / 2 - 10 - tip.current.offsetWidth : right)
  }, [hover, slot, W])
  const mid = Math.floor((days.length - 1) / 2)
  const xl = ([[0, 'start'], [mid, 'middle'], [days.length - 1, 'end']] as [number, 'start' | 'middle' | 'end'][])
    .filter(([i], n, a) => a.findIndex(z => z[0] === i) === n)
  const hd = hover != null ? days[hover] : null
  return (
    <div className="uchartwrap" ref={wrap}>
      <svg viewBox={`0 0 ${W} ${H}`} className="uchart" role="img" aria-label={`Uses per day for the last ${days.length} days, stacked by kind`}
        onPointerLeave={() => setHover(null)}>
        {[0, max / 2, max].map(v => [
          <line key={`l${v}`} x1={L} x2={W - R} y1={y(v)} y2={y(v)} className={v ? 'ugrid' : 'ubase'} />,
          <text key={`t${v}`} x={L - 8} y={y(v) + 4} className="utick" textAnchor="end">{Number.isInteger(v) ? v : v.toFixed(1)}</text>,
        ])}
        <rect className="uhover" x={hover != null ? L + hover * slot : 0} y={T} width={slot} height={ph} visibility={hover != null ? 'visible' : 'hidden'} />
        {days.map((d, i) => {
          const x = L + i * slot + (slot - bw) / 2
          let base = T + ph
          const segs = KINDS.filter(([k]) => d[k])
          return segs.map(([k], j) => {
            const hgt = (d[k] / max) * ph
            const top = base - hgt
            const gap = j > 0 && hgt > 3 ? 2 : 0 // a thin surface gap between stacked kinds
            base = top
            return j === segs.length - 1
              ? <path key={`${i}${k}`} d={topRounded(x, top, bw, hgt - gap, 3)} className={`ubar s-${k}`} />
              : <rect key={`${i}${k}`} x={x} y={top} width={bw} height={Math.max(0, hgt - gap)} className={`ubar s-${k}`} />
          })
        })}
        {xl.map(([i, anchor]) => (
          <text key={`x${i}`} x={anchor === 'start' ? L : anchor === 'end' ? W - R : L + (i + 0.5) * slot} y={H - 6} className="utick" textAnchor={anchor}>
            {i === days.length - 1 ? 'Today' : dayDate(days[i].d, { month: 'short', day: 'numeric' })}
          </text>
        ))}
        {days.map((_, i) => <rect key={`h${i}`} x={L + i * slot} y={T} width={slot} height={ph} className="uhit" onPointerEnter={() => setHover(i)} />)}
      </svg>
      <div className="utip" role="status" hidden={!hd} ref={tip} style={{ left: `${tipLeft}px` }}>
        {hd ? (
          <>
            <div className="utipd">{dayLabel(hd.d, s.today)}</div>
            {KINDS.map(([k]) => <div key={k} className={`utipr${hd[k] ? '' : ' zero'}`}><Swatch k={k} /><span>{KIND_TITLE[k]}</span><b>{hd[k]}</b></div>)}
            {hd.failed ? <div className="utipr bad"><span>Failed</span><b>{hd.failed}</b></div> : null}
          </>
        ) : null}
      </div>
    </div>
  )
}

function Table({ s }: { s: Stats }) {
  return (
    <div className="utable" role="table" aria-label="Uses per day">
      <div className="utr head" role="row">
        <span role="columnheader">Day</span>
        {KINDS.map(([k]) => <span key={k} role="columnheader">{KIND_TITLE[k]}</span>)}
        <span role="columnheader">Failed</span>
      </div>
      {[...s.daily].reverse().map(d => (
        <div key={d.d} className="utr" role="row">
          <span role="cell">{dayLabel(d.d, s.today)}</span>
          {KINDS.map(([k]) => <span key={k} role="cell">{d[k] || '·'}</span>)}
          <span role="cell" className={d.failed ? 't-bad' : undefined}>{d.failed || '·'}</span>
        </div>
      ))}
    </div>
  )
}

function Sparkline({ values }: { values: number[] }) {
  const W = 84, H = 22, max = Math.max(1, ...values)
  const pts = values.map((v, i) => `${(i / Math.max(1, values.length - 1)) * (W - 2) + 1},${H - 2 - (v / max) * (H - 4)}`).join(' ')
  return <svg viewBox={`0 0 ${W} ${H}`} className="uspark" aria-hidden="true"><polyline points={pts} /></svg>
}

function ago(ts: number) {
  if (!ts) return 'never'
  const s = Date.now() / 1000 - ts
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`
  if (s < 86400) return `${Math.round(s / 3600)} h ago`
  const d = Math.round(s / 86400)
  return d === 1 ? 'yesterday' : `${d} days ago`
}

export function Usage({ width }: { width: number }) {
  const s = store.use(st => st.stats)
  const utable = store.use(st => st.utable)
  const list = store.use(st => st.list)
  if (!s) return <div className="empty">Adding up keymule’s activity log…</div>
  const sums = Object.fromEntries(KINDS.map(([k]) => [k, s.daily.reduce((a, d) => a + d[k], 0)])) as Record<UseKind, number>
  const failed = s.daily.reduce((a, d) => a + d.failed, 0)
  const total = Object.values(sums).reduce((a, b) => a + b, 0)
  const used = s.keys.filter(k => k.total)
  const known = [...(list?.credentials || []), ...(list?.servers || [])].map(c => c.name)
  const byName = Object.fromEntries(s.keys.map(k => [k.name, k]))
  const idle = known.filter(n => !byName[n]?.total).map(n => ({ name: n, last: byName[n]?.last || 0 })).sort((a, b) => b.last - a.last)
  return (
    <>
      <div className="ush">
        <h3>{total.toLocaleString()} uses in the last {s.days} days</h3>
        <span className="grow" />
        <div className="seg" role="group" aria-label="Time range">
          {[7, 30, 90].map(n => <button key={n} type="button" className={s.days === n ? 'on' : undefined} aria-pressed={s.days === n} onClick={() => setRange(n)}>{n} days</button>)}
        </div>
      </div>
      <div className="ulegend">
        {KINDS.map(([k]) => <div key={k} className="ul"><span className="k"><Swatch k={k} />{KIND_TITLE[k]}</span><span className="v">{sums[k].toLocaleString()}</span></div>)}
        <div className="ul"><span className="k">Failed</span><span className={`v ${failed ? 't-bad' : ''}`}>{failed.toLocaleString()}</span></div>
      </div>
      {total ? <Chart s={s} width={width} /> : <div className="empty">No key was used in the last {s.days} days.</div>}
      {total ? (
        <div className="umore">
          <button className="btn link" type="button" onClick={() => store.set({ utable: !utable })}>{utable ? 'Hide the table' : 'Show as a table'}</button>
          <span className="muted">Commands and server starts are counted when they start, so only API calls and GitHub uploads can fail here.</span>
        </div>
      ) : null}
      {utable && total ? <Table s={s} /> : null}
      <div className="ush"><h3>By key</h3></div>
      {used.length ? (
        <div className="ukeys">
          {used.map(k => (
            <div key={k.name} className="ukey">
              <span className="nm mono">{k.name}</span>
              <Sparkline values={k.spark} />
              <span className="split"><Join parts={KINDS.filter(([c]) => k[c]).map(([c, one, many]) => <span className="part"><Swatch k={c} />{plural(k[c], one, many)}</span>)} /></span>
              <span className="tot">{k.total.toLocaleString()}{k.failed ? <span className="t-bad"> · {k.failed} failed</span> : null}</span>
              <span className="last">{ago(k.last)}</span>
            </div>
          ))}
        </div>
      ) : <div className="empty">No key was used in this range.</div>}
      {idle.length ? (
        <>
          <div className="ush"><h3>Not used in the last {s.days} days</h3><span className="count">{idle.length}</span></div>
          <p className="note">Stored but idle. If you no longer need one, rotate or remove it: a key nobody uses is still a key that can leak.</p>
          <div className="ukeys idle">
            {idle.map(k => <div key={k.name} className="ukey"><span className="nm mono">{k.name}</span><span className="last">{k.last ? `last used ${ago(k.last)}` : 'never used'}</span></div>)}
          </div>
        </>
      ) : null}
    </>
  )
}
