// Small shared pieces: icons, badges, and joining parts with " · ".
import { Fragment, type ReactNode } from 'react'

type Shape = [string, Record<string, string | number>]

// Stroke icons, 24x24.
const ICONS: Record<string, Shape[]> = {
  key: [['circle', { cx: 7.5, cy: 15.5, r: 4.5 }], ['path', { d: 'M10.7 12.3 20 3m-3 3 3 3m-6 0 2 2' }]],
  server: [['rect', { x: 3, y: 3.5, width: 18, height: 7, rx: 2 }], ['rect', { x: 3, y: 13.5, width: 18, height: 7, rx: 2 }], ['path', { d: 'M7 7h.01M7 17h.01' }]],
  warn: [['path', { d: 'M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z' }], ['path', { d: 'M12 9v4M12 17h.01' }]],
  eye: [['path', { d: 'M1.5 12S5.5 5 12 5s10.5 7 10.5 7-4 7-10.5 7S1.5 12 1.5 12z' }], ['circle', { cx: 12, cy: 12, r: 3 }]],
  eyeOff: [['path', { d: 'M10.7 5.1A10.7 10.7 0 0 1 22.5 12a17 17 0 0 1-2 2.9M14.1 14.2a3 3 0 0 1-4.2-4.2M17.5 17.5A10.5 10.5 0 0 1 1.5 12a17 17 0 0 1 5-5.5M2 2l20 20' }]],
  check: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'm8 12.5 2.5 2.5L16 9.5' }]],
  x: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'm15 9-6 6M9 9l6 6' }]],
  shield: [['path', { d: 'M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z' }], ['path', { d: 'm9 12 2 2 4-4' }]],
  spin: [['path', { d: 'M21 12a9 9 0 1 1-6.2-8.56' }]],
  home: [['rect', { x: 3, y: 3, width: 7, height: 9, rx: 1.5 }], ['rect', { x: 14, y: 3, width: 7, height: 5, rx: 1.5 }], ['rect', { x: 14, y: 12, width: 7, height: 9, rx: 1.5 }], ['rect', { x: 3, y: 16, width: 7, height: 5, rx: 1.5 }]],
  pulse: [['path', { d: 'M3 12h4l3-8 4 16 3-8h4' }]],
  sidebar: [['rect', { x: 3, y: 4, width: 18, height: 16, rx: 2.5 }], ['path', { d: 'M9 4v16' }]],
  sun: [['circle', { cx: 12, cy: 12, r: 4 }], ['path', { d: 'M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4' }]],
  moon: [['path', { d: 'M20.5 14.1A8.5 8.5 0 1 1 9.9 3.5a6.6 6.6 0 0 0 10.6 10.6z' }]],
  globe: [['circle', { cx: 12, cy: 12, r: 9.5 }], ['path', { d: 'M2.5 12h19M12 2.5c3.2 3.4 3.2 15.6 0 19M12 2.5c-3.2 3.4-3.2 15.6 0 19' }]],
  terminal: [['rect', { x: 2.5, y: 4, width: 19, height: 16, rx: 2 }], ['path', { d: 'm7 9 3 3-3 3M13 15h4' }]],
  monitor: [['rect', { x: 2.5, y: 4, width: 19, height: 13, rx: 2 }], ['path', { d: 'M8 21h8M12 17v4' }]],
  more: [['circle', { cx: 5, cy: 12, r: 1 }], ['circle', { cx: 12, cy: 12, r: 1 }], ['circle', { cx: 19, cy: 12, r: 1 }]],
  chev: [['path', { d: 'm9 6 6 6-6 6' }]],
  pen: [['path', { d: 'M12 20h9' }], ['path', { d: 'M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z' }]],
  chart: [['path', { d: 'M3 3v18h18' }], ['path', { d: 'M8 17v-5M13 17V8M18 17v-9' }]],
}
export type IconName = keyof typeof ICONS

export function Icon({ name }: { name: IconName }) {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      {(ICONS[name] || []).map(([Tag, attrs], i) => {
        const El = Tag as 'path'
        return <El key={i} {...(attrs as object)} />
      })}
    </svg>
  )
}

export const Ico = ({ name, cls }: { name: IconName; cls?: string }) => (
  <span className={`ico ${cls || ''}${name === 'spin' ? ' spinning' : ''}`} aria-hidden="true"><Icon name={name} /></span>
)

export const Badge = ({ kind, children }: { kind?: string; children: ReactNode }) => (
  <span className={`badge ${kind || ''}`}>{children}</span>
)

export const Mono = ({ children }: { children: ReactNode }) => <span className="mono">{children}</span>

// Parts with " · " between them; empty parts are dropped.
export function Join({ parts }: { parts: ReactNode[] }) {
  const kept = parts.filter(p => p != null && p !== false && p !== '')
  return <>{kept.map((p, i) => <Fragment key={i}>{i ? ' · ' : null}{p}</Fragment>)}</>
}

export function when(ts: number) {
  const d = new Date(ts * 1000)
  const now = new Date()
  if (d.toDateString() === now.toDateString()) return d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })
  return d.toLocaleDateString([], { month: 'short', day: 'numeric' })
}

export const plural = (n: number, one: string, many: string) => `${n.toLocaleString()} ${n === 1 ? one : many}`
