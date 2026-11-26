import { api } from '../lib/api'
import { isBusy, isOpen, run, store, toast, type ConvertForm } from '../lib/app'
import type { ServerSecret } from '../lib/types'
import { Details, MoreMenu, Paged, RevealButton, Row, RowTitle, useCommonItems, useFirstRows } from './rows'
import { Badge, Ico, Join, Mono } from './ui'

function ServerRow({ s }: { s: ServerSecret }) {
  const open = store.use(st => isOpen(st, s.name))
  const converting = store.use(st => st.converting === s.name)
  const common = useCommonItems(s.name, s.description)
  const what = [...(s.env || []), ...(s.headers || []).map(x => `${x} header`)]
  if (s.arg_secrets) what.push(`${s.arg_secrets} in arguments`)
  const used = s.used_by.length
  const single = what.length === 1 && !s.arg_secrets
  const row = (
    <Row name={s.name}>
      <Ico name="server" />
      <span className="body">
        <RowTitle name={s.name} />
        <span className={open ? 'sub wrap' : 'sub'}>
          <Join parts={[<Mono>{what.join(', ')}</Mono>, used ? `used by ${used} config entr${used === 1 ? 'y' : 'ies'}` : 'no MCP server runs it']} />
        </span>
        {open ? <Details name={s.name} text={s.description} /> : null}
      </span>
      <span className="acc">
        {used ? <Badge kind="ok">Active</Badge> : <Badge>Unused</Badge>}
        <RevealButton name={s.name} />
        <MoreMenu name={s.name} items={[
          !used && single ? ['Move to Credentials', () => store.set(st => ({
            converting: st.converting === s.name ? null : s.name,
            convertForm: st.converting === s.name ? null : { to: 'ssh', host: '', username: '', port: '', description: s.description || '' },
          })), { title: 'Not an MCP server secret? Make it an SSH login or a password, without retyping it' }] : null,
          ...common,
        ]} />
      </span>
    </Row>
  )
  return converting ? <div className="rowgroup">{row}<Convert s={s} /></div> : row
}

// Move an unused MCP server secret to Credentials as an SSH login or a
// password. The value never comes here: keymule moves it, after a yes in its
// own window.
function Convert({ s }: { s: ServerSecret }) {
  const f = store.use(st => st.convertForm) || { to: 'ssh', host: '', username: '', port: '', description: s.description || '' }
  const busy = store.use(() => isBusy(`convert:${s.name}`))
  const setF = (p: Partial<ConvertForm>) => store.set(st => ({ convertForm: { ...f, ...st.convertForm, ...p } }))
  const close = () => store.set({ converting: null, convertForm: null })
  const ssh = f.to === 'ssh'
  const field = (key: 'host' | 'username' | 'port' | 'description', cls: string, placeholder: string, label: string, extra = {}) => (
    <input className={cls} type="text" spellCheck={false} placeholder={placeholder} aria-label={label}
      value={f[key]} onChange={e => setF({ [key]: e.target.value })} {...extra} />
  )
  return (
    <form className="convert" onSubmit={e => {
      e.preventDefault()
      if (ssh && (!f.host.trim() || !f.username.trim())) { toast('Give the server\'s host or IP and the username.', 'bad'); return }
      const body = { name: s.name, to: f.to, host: f.host.trim(), username: f.username.trim(), port: f.port.trim(), description: f.description.trim() }
      run(`convert:${s.name}`, `A keymule window asks before moving "${s.name}".`, () => api('/api/convert', body), close)
    }}>
      <p className="note">Move "{s.name}" to Credentials. keymule moves the value itself: nobody sees it or types it again.</p>
      <div className="addline">
        <select className="field" aria-label="Move it as" value={f.to} onChange={e => setF({ to: e.target.value as ConvertForm['to'] })}>
          <option value="ssh">SSH login</option>
          <option value="password">Password for programs</option>
        </select>
        {ssh ? field('host', 'field url', 'Host or IP, e.g. 203.0.113.7', 'Host or IP') : null}
        {ssh ? field('username', 'field short', 'Username', 'Username') : null}
        {ssh ? field('port', 'field port', '22', 'Port', { inputMode: 'numeric' }) : null}
      </div>
      <div className="addline">
        {field('description', 'field url', 'What it is for (optional)', 'Description', { maxLength: 300 })}
        <button className="btn" type="submit" disabled={busy}>{busy ? 'Waiting…' : 'Move'}</button>
        <button className="btn ghost" type="button" onClick={close}>Cancel</button>
      </div>
    </form>
  )
}

export function ServerList() {
  const servers = store.use(s => s.list?.servers)
  const first = useFirstRows(5)
  if (!servers) return null
  if (!servers.length) return <div className="empty">No MCP server secrets yet. Plain-text keys in your agent configs show up above, ready to move.</div>
  return <Paged rows={servers} id="servers" base={first} render={s => <ServerRow key={s.name} s={s} />} />
}
