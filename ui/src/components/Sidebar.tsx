import { go, nextTheme, store, toggleSidebar, type Theme, type View } from '../lib/app'
import { Icon, type IconName } from './ui'

export const VIEWS: [View, string, IconName, string][] = [
  ['overview', 'Overview', 'home', 'Your agents use these keys without ever seeing them. Everything stays on this machine.'],
  ['credentials', 'Credentials', 'key', 'API tokens, passwords and SSH logins your agents use through keymule. You type each one into a keymule dialog.'],
  ['servers', 'MCP servers', 'server', 'Secrets keymule hands to your MCP servers when they start, instead of keeping them in config files.'],
  ['leaks', 'Leak check', 'shield', 'Keys that already reached a model, sit in plain text, or are readable by agents.'],
  ['usage', 'Usage', 'chart', 'How often your agents used each key, and how: API calls, commands, MCP servers, and values shown to you.'],
  ['activity', 'Activity', 'pulse', 'Every request and server start that used a key, newest first.'],
]

const THEMES: Record<Theme, [IconName, string]> = { system: ['monitor', 'System theme'], light: ['sun', 'Light theme'], dark: ['moon', 'Dark theme'] }

export function Sidebar() {
  const view = store.use(s => s.view)
  const list = store.use(s => s.list)
  const doctor = store.use(s => s.doctor)
  const collapsed = store.use(s => s.collapsed)
  const theme = store.use(s => s.theme)
  const live = store.use(s => s.live)
  const forbidden = store.use(s => s.forbidden)
  const version = store.use(s => s.version)
  const leaks = doctor && doctor !== 'running' && 'entries' in doctor ? doctor.entries.filter(e => !e.stored_as).length : null
  const counts: Partial<Record<View, number | null>> = { credentials: list?.credentials.length, servers: list?.servers.length, leaks }
  const [ic, label] = THEMES[theme] || THEMES.system
  const what = collapsed ? 'Expand sidebar' : 'Collapse sidebar'
  return (
    <aside className="sidebar">
      <div className="brandline">
        <svg className="logo" viewBox="0 0 24 24" aria-hidden="true"><rect width="24" height="24" rx="6" className="logo-bg" /><circle cx="12" cy="9.6" r="3.3" className="logo-fg" /><path d="M10.3 11.8 9.4 17.6h5.2l-.9-5.8z" className="logo-fg" /></svg>
        <span className="brand">keymule</span>
        <button id="collapse" className="iconbtn" type="button" title={what} aria-label={what} onClick={toggleSidebar}><Icon name="sidebar" /></button>
      </div>
      <nav id="nav" className="nav" aria-label="Sections">
        {VIEWS.map(([id, label, icon]) => (
          <button key={id} type="button" className={view === id ? 'on' : undefined} aria-current={view === id ? 'page' : undefined}
            onClick={() => go(id)} title={collapsed ? label : undefined}>
            <Icon name={icon} /><span className="label">{label}</span>
            {counts[id] != null ? <span className={`n ${id === 'leaks' && counts[id] ? 'bad' : ''}`}>{counts[id]}</span> : null}
          </button>
        ))}
      </nav>
      <div className="sidefoot">
        <button id="theme" className="navbtn" type="button" title={`${label} (click to change)`} onClick={nextTheme}>
          <Icon name={ic} /><span className="label">{label}</span>
        </button>
        <div className="status">
          <span id="live" className={`live${live === false ? ' off' : ''}`}>
            {live === false
              ? <><span className="dot" />{forbidden ? 'No access: open the link `keymule ui` printed' : 'Disconnected: run `keymule ui` again'}</>
              : live ? <><span className="dot" />Live</> : null}
          </span>
          <span id="version" className="muted">{version ? `v${version}` : ''}</span>
        </div>
      </div>
    </aside>
  )
}
