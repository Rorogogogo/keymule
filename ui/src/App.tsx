// keymule ui. Everything shown here comes from keymule commands that never
// print secrets; buttons run commands that open keymule's own dialogs.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import { answer, closeRevealed, go, hideAsk, paired, startScan, store, type View } from './lib/app'
import { Activity, FilterChip } from './components/Activity'
import { AskCard, RevealCard, Toast } from './components/Ask'
import { Callout, PairingBanners, StatTiles, UpdateBanner } from './components/Banners'
import { AddForm, CredList } from './components/Credentials'
import { Leaks } from './components/Leaks'
import { ServerList } from './components/Servers'
import { Sidebar, VIEWS } from './components/Sidebar'
import { Usage } from './components/Usage'

// A section shows in the views listed in `in`; hidden (not removed) in the
// others, which the CSS's column rules rely on.
function Sec({ views, children }: { views: View[]; children: ReactNode }) {
  const view = store.use(s => s.view)
  return <section className="sec" hidden={!views.includes(view)}>{children}</section>
}

// The page's global keys and clicks.
function useGlobalHandlers() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return
      const s = store.get()
      if (s.revealed) { closeRevealed(); return }
      if (s.ap.busy) return
      const item = s.ap.live ? s.ap.pending.find(p => !s.ap.hidden.includes(p.req.id)) : undefined
      if (!item) return
      e.preventDefault()
      if (paired(s)) answer(item, item.req.spec.cancel)
      else hideAsk(item)
    }
    const onClick = (e: MouseEvent) => {
      if (store.get().menu && !(e.target as Element).closest?.('.menuwrap')) store.set({ menu: null })
    }
    document.addEventListener('keydown', onKey)
    document.addEventListener('click', onClick)
    return () => { document.removeEventListener('keydown', onKey); document.removeEventListener('click', onClick) }
  }, [])
  const theme = store.use(s => s.theme)
  useEffect(() => {
    if (theme === 'system') delete document.documentElement.dataset.theme
    else document.documentElement.dataset.theme = theme
  }, [theme])
}

// Scroll position as a thin bar along the top, in place of a scrollbar.
function Progress() {
  const bar = useRef<HTMLSpanElement>(null)
  useEffect(() => {
    const update = () => {
      const s = document.scrollingElement!
      const max = s.scrollHeight - s.clientHeight
      if (bar.current) bar.current.style.transform = `scaleX(${max > 0 ? Math.min(1, s.scrollTop / max) : 0})`
    }
    update()
    const ro = new ResizeObserver(update)
    ro.observe(document.body)
    addEventListener('scroll', update, { passive: true })
    addEventListener('resize', update)
    return () => { ro.disconnect(); removeEventListener('scroll', update); removeEventListener('resize', update) }
  }, [])
  return <div id="progress" className="progress" aria-hidden="true"><span ref={bar} /></div>
}

// The usage chart is drawn at its container's width, redrawn in 40 px steps.
function useWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null)
  const [width, setWidth] = useState(0)
  useEffect(() => {
    if (!ref.current) return
    const ro = new ResizeObserver(([e]) => {
      const w = e.contentRect.width
      setWidth(old => (Math.round(old / 40) === Math.round(w / 40) && old ? old : w))
    })
    ro.observe(ref.current)
    return () => ro.disconnect()
  }, [])
  return [ref, width] as const
}

export function App() {
  useGlobalHandlers()
  const view = store.use(s => s.view)
  const collapsed = store.use(s => s.collapsed)
  const ncreds = store.use(s => s.list?.credentials.length)
  const nservers = store.use(s => s.list?.servers.length)
  const doctor = store.use(s => s.doctor)
  const [usageRef, usageWidth] = useWidth<HTMLDivElement>()
  const v = VIEWS.find(x => x[0] === view) || VIEWS[0]
  return (
    <>
      <Progress />
      <div className={`app${collapsed ? ' collapsed' : ''}`}>
        <Sidebar />
        <main id="main" className={`main v-${v[0]}`}>
          <header className="ph">
            <div>
              <h1 id="ptitle">{v[1]}</h1>
              <p id="psub">{v[3]}</p>
            </div>
          </header>

          <div id="update"><UpdateBanner /></div>
          <div id="pairing"><PairingBanners /></div>
          <div id="stats" className="stats" hidden={view !== 'overview'}><StatTiles /></div>
          <div id="banner" hidden={!['overview', 'credentials', 'servers'].includes(view)}><Callout /></div>

          <div className="grid">
            <div className="colA">
              <Sec views={['usage']}>
                <div id="usage" ref={usageRef}>{view === 'usage' ? <Usage width={usageWidth} /> : null}</div>
              </Sec>

              <Sec views={['overview', 'credentials']}>
                <div className="sh"><h2>Credentials</h2><span id="ncreds" className="count">{ncreds || ''}</span></div>
                <div id="creds" className="table"><CredList /></div>
                <AddForm />
              </Sec>

              <Sec views={['overview', 'servers']}>
                <div className="sh"><h2>MCP servers</h2><span id="nservers" className="count">{nservers || ''}</span></div>
                <div id="servers" className="table"><ServerList /></div>
              </Sec>
            </div>

            <div className="colB">
              <Sec views={['overview', 'activity']}>
                <div className="sh">
                  <h2>Activity</h2>
                  <span id="filter" className="grow"><FilterChip /></span>
                  <button id="allact" className="btn link" type="button" hidden={view !== 'overview'} onClick={() => go('activity')}>View all</button>
                </div>
                <div id="activity" className="table acts"><Activity /></div>
              </Sec>

              <Sec views={['overview', 'leaks']}>
                <div className="sh">
                  <h2>Leak check</h2>
                  <span className="grow" />
                  <button id="scan" className="btn" type="button" disabled={doctor === 'running'} onClick={startScan}>
                    {doctor === 'running' ? 'Scanning…' : doctor ? 'Scan again' : 'Scan'}
                  </button>
                </div>
                <div id="leaks"><Leaks /></div>
              </Sec>
            </div>
          </div>
        </main>
      </div>
      <AskCard />
      <RevealCard />
      <Toast />
    </>
  )
}
