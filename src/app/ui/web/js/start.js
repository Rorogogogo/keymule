// Scroll position as a thin bar along the top, in place of a scrollbar.
function updateProgress() {
  const s = document.scrollingElement
  const max = s.scrollHeight - s.clientHeight
  $('progress').firstElementChild.style.transform = `scaleX(${max > 0 ? Math.min(1, s.scrollTop / max) : 0})`
}
addEventListener('scroll', updateProgress, { passive: true })
addEventListener('resize', updateProgress)
let resizeTimer
addEventListener('resize', () => { clearTimeout(resizeTimer); resizeTimer = setTimeout(render, 150) })

render()
refresh()
loadStats()
loadPlan()
pollScan()
setInterval(refresh, 20000)
let statsKey = ''
setInterval(async () => {
  try {
    const r = await api('/api/audit')
    state.audit = r.audit || []
    state.live = true
    const last = state.audit[state.audit.length - 1]
    const key = `${state.audit.length}|${last?.ts}|${new Date().toDateString()}`
    if (key !== statsKey) { statsKey = key; loadStats() }
  } catch (e) {
    state.live = false
    state.forbidden = e.message === 'forbidden'
  }
  render()
}, 3000)
setInterval(loadPlan, 60000)

