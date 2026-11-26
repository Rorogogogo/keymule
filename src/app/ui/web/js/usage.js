// ---- usage ---------------------------------------------------------------------

const NS = 'http://www.w3.org/2000/svg'
function sv(tag, attrs, ...kids) {
  const el = document.createElementNS(NS, tag)
  for (const [k, v] of Object.entries(attrs || {})) if (v != null) el.setAttribute(k, v)
  for (const kid of kids.flat(Infinity)) if (kid != null) el.append(kid instanceof Node ? kid : String(kid))
  return el
}

// Day numbers count days since 1970 in the viewer's time zone.
const dayDate = (d, opts) => new Date(d * 86400000).toLocaleDateString([], { timeZone: 'UTC', ...opts })
const dayLabel = (d, today) => d === today ? 'Today' : d === today - 1 ? 'Yesterday' : dayDate(d, { weekday: 'short', month: 'short', day: 'numeric' })

// A round top for the axis whose half is a whole number too.
function niceMax(v) {
  if (v <= 4) return 4
  const p = 10 ** Math.floor(Math.log10(v))
  return [1, 2, 3, 4, 5, 6, 8, 10].map(m => m * p).find(m => m >= v && m % 2 === 0)
}

// A bar with rounded top corners, square at the baseline.
function topRounded(x, y, w, hgt, r) {
  r = Math.min(r, w / 2, hgt)
  return `M${x},${y + hgt}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + hgt}Z`
}

function usageChart(s, width) {
  // Drawn at the real width, so text and bars stay their true size.
  const W = Math.max(300, width || 720), H = 220, L = 34, R = 6, T = 10, B = 24
  const pw = W - L - R, ph = H - T - B
  const days = s.daily
  const max = niceMax(Math.max(1, ...days.map(d => d.total)))
  const slot = pw / days.length
  const bw = Math.max(2, Math.min(28, slot * 0.64))
  const y = v => T + ph - (v / max) * ph
  const grid = [0, max / 2, max].map(v => [
    sv('line', { x1: L, x2: W - R, y1: y(v), y2: y(v), class: v ? 'ugrid' : 'ubase' }),
    sv('text', { x: L - 8, y: y(v) + 4, class: 'utick', 'text-anchor': 'end' }, Number.isInteger(v) ? v : v.toFixed(1)),
  ])
  const bars = days.map((d, i) => {
    const x = L + i * slot + (slot - bw) / 2
    let base = T + ph
    const segs = KINDS.filter(([k]) => d[k])
    return segs.map(([k], j) => {
      const hgt = (d[k] / max) * ph
      const top = base - hgt
      const gap = j > 0 && hgt > 3 ? 2 : 0 // a thin surface gap between stacked kinds
      const el = j === segs.length - 1
        ? sv('path', { d: topRounded(x, top, bw, hgt - gap, 3), class: `ubar s-${k}` })
        : sv('rect', { x, y: top, width: bw, height: Math.max(0, hgt - gap), class: `ubar s-${k}` })
      base = top
      return el
    })
  })
  const mid = Math.floor((days.length - 1) / 2)
  const xl = [[0, 'start'], [mid, 'middle'], [days.length - 1, 'end']].filter(([i], n, a) => a.findIndex(z => z[0] === i) === n)
  const xlabels = xl.map(([i, anchor]) => sv('text', {
    x: anchor === 'start' ? L : anchor === 'end' ? W - R : L + (i + 0.5) * slot, y: H - 6, class: 'utick', 'text-anchor': anchor,
  }, i === days.length - 1 ? 'Today' : dayDate(days[i].d, { month: 'short', day: 'numeric' })))
  const tip = h('div', { class: 'utip', hidden: true, role: 'status' })
  const hover = sv('rect', { class: 'uhover', x: 0, y: T, width: slot, height: ph, visibility: 'hidden' })
  const hits = days.map((d, i) => {
    const r = sv('rect', { x: L + i * slot, y: T, width: slot, height: ph, class: 'uhit' })
    r.addEventListener('pointerenter', () => {
      hover.setAttribute('x', L + i * slot); hover.setAttribute('visibility', 'visible')
      tip.replaceChildren(...[
        h('div', { class: 'utipd' }, dayLabel(d.d, s.today)),
        KINDS.map(([k]) => h('div', { class: `utipr${d[k] ? '' : ' zero'}` }, swatch(k), h('span', null, KIND_TITLE[k]), h('b', null, d[k]))),
        d.failed ? h('div', { class: 'utipr bad' }, h('span', null, 'Failed'), h('b', null, d.failed)) : null,
      ].flat().filter(Boolean))
      tip.hidden = false
      // Beside the bar: to its right, or to its left near the right edge.
      const wrap = tip.parentElement
      const px = ((L + (i + 0.5) * slot) / W) * wrap.clientWidth
      const right = px + slot / 2 + 10
      tip.style.left = `${right + tip.offsetWidth > wrap.clientWidth ? px - slot / 2 - 10 - tip.offsetWidth : right}px`
    })
    return r
  })
  const svg = sv('svg', { viewBox: `0 0 ${W} ${H}`, class: 'uchart', role: 'img', 'aria-label': `Uses per day for the last ${days.length} days, stacked by kind` },
    grid, hover, bars, xlabels, hits)
  svg.addEventListener('pointerleave', () => { tip.hidden = true; hover.setAttribute('visibility', 'hidden') })
  return h('div', { class: 'uchartwrap' }, svg, tip)
}

function usageTable(s) {
  return h('div', { class: 'utable', role: 'table', 'aria-label': 'Uses per day' },
    h('div', { class: 'utr head', role: 'row' }, h('span', { role: 'columnheader' }, 'Day'), KINDS.map(([k]) => h('span', { role: 'columnheader' }, KIND_TITLE[k])), h('span', { role: 'columnheader' }, 'Failed')),
    [...s.daily].reverse().map(d => h('div', { class: 'utr', role: 'row' },
      h('span', { role: 'cell' }, dayLabel(d.d, s.today)),
      KINDS.map(([k]) => h('span', { role: 'cell' }, d[k] || '·')),
      h('span', { role: 'cell', class: d.failed ? 't-bad' : null }, d.failed || '·'))))
}

function sparkline(values) {
  const W = 84, H = 22, max = Math.max(1, ...values)
  const pts = values.map((v, i) => `${(i / Math.max(1, values.length - 1)) * (W - 2) + 1},${H - 2 - (v / max) * (H - 4)}`).join(' ')
  return sv('svg', { viewBox: `0 0 ${W} ${H}`, class: 'uspark', 'aria-hidden': 'true' }, sv('polyline', { points: pts }))
}

function ago(ts) {
  if (!ts) return 'never'
  const s = Date.now() / 1000 - ts
  if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`
  if (s < 86400) return `${Math.round(s / 3600)} h ago`
  const d = Math.round(s / 86400)
  return d === 1 ? 'yesterday' : `${d} days ago`
}

function renderUsage() {
  const s = state.stats
  if (!s) return [h('div', { class: 'empty' }, 'Adding up keymule’s activity log…')]
  const sums = Object.fromEntries(KINDS.map(([k]) => [k, s.daily.reduce((a, d) => a + d[k], 0)]))
  const failed = s.daily.reduce((a, d) => a + d.failed, 0)
  const total = Object.values(sums).reduce((a, b) => a + b, 0)
  const range = h('div', { class: 'seg', role: 'group', 'aria-label': 'Time range' },
    [7, 30, 90].map(n => h('button', { type: 'button', class: s.days === n ? 'on' : null, 'aria-pressed': String(s.days === n), onclick: () => setRange(n) }, `${n} days`)))
  const legend = h('div', { class: 'ulegend' },
    KINDS.map(([k]) => h('div', { class: 'ul' }, h('span', { class: 'k' }, swatch(k), KIND_TITLE[k]), h('span', { class: 'v' }, sums[k].toLocaleString()))),
    h('div', { class: 'ul' }, h('span', { class: 'k' }, 'Failed'), h('span', { class: `v ${failed ? 't-bad' : ''}` }, failed.toLocaleString())))
  const used = s.keys.filter(k => k.total)
  const known = [...(state.list?.credentials || []), ...(state.list?.servers || [])].map(c => c.name)
  const byName = Object.fromEntries(s.keys.map(k => [k.name, k]))
  const idle = known.filter(n => !byName[n]?.total).map(n => ({ name: n, last: byName[n]?.last || 0 })).sort((a, b) => b.last - a.last)
  return [
    h('div', { class: 'ush' }, h('h3', null, `${total.toLocaleString()} uses in the last ${s.days} days`), h('span', { class: 'grow' }), range),
    legend,
    total ? usageChart(s, $('usage').clientWidth) : h('div', { class: 'empty' }, `No key was used in the last ${s.days} days.`),
    total ? h('div', { class: 'umore' },
      h('button', { class: 'btn link', type: 'button', onclick: () => { state.utable = !state.utable; render() } }, state.utable ? 'Hide the table' : 'Show as a table'),
      h('span', { class: 'muted' }, 'Commands and server starts are counted when they start, so only API calls and GitHub uploads can fail here.')) : null,
    state.utable && total ? usageTable(s) : null,
    h('div', { class: 'ush' }, h('h3', null, 'By key')),
    used.length ? h('div', { class: 'ukeys' }, used.map(k => h('div', { class: 'ukey' },
      h('span', { class: 'nm mono' }, k.name),
      sparkline(k.spark),
      h('span', { class: 'split' }, join(KINDS.filter(([c]) => k[c]).map(([c, one, many]) => h('span', { class: 'part' }, swatch(c), plural(k[c], one, many))))),
      h('span', { class: 'tot' }, k.total.toLocaleString(), k.failed ? h('span', { class: 't-bad' }, ` · ${k.failed} failed`) : null),
      h('span', { class: 'last' }, ago(k.last)))))
      : h('div', { class: 'empty' }, 'No key was used in this range.'),
    idle.length ? [
      h('div', { class: 'ush' }, h('h3', null, `Not used in the last ${s.days} days`), h('span', { class: 'count' }, idle.length)),
      h('p', { class: 'note' }, 'Stored but idle. If you no longer need one, rotate or remove it: a key nobody uses is still a key that can leak.'),
      h('div', { class: 'ukeys idle' }, idle.map(k => h('div', { class: 'ukey' }, h('span', { class: 'nm mono' }, k.name), h('span', { class: 'last' }, k.last ? `last used ${ago(k.last)}` : 'never used')))),
    ] : null,
  ].flat().filter(Boolean)
}

