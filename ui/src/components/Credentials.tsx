import { useState, type ReactNode } from 'react'
import { api } from '../lib/api'
import { isBusy, isOpen, run, store, toast } from '../lib/app'
import type { Credential } from '../lib/types'
import { Details, MoreMenu, Paged, RevealButton, Row, RowTitle, useCommonItems, useFirstRows } from './rows'
import { Badge, Ico, Join, Mono } from './ui'

// How passwords, keys and AWS pairs are used: only by programs, never HTTP.
const USE_HINT: Record<string, string> = {
  password: 'for programs: keymule exec --env VAR',
  'private-key': 'for programs, as a file: keymule exec --file KEY',
  aws: 'for programs: keymule exec (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)',
}

// The line under a credential's name, as parts (and as plain text for its tooltip).
function credSub(c: Credential): [ReactNode, string][] {
  const t = (s: string): [ReactNode, string] => [s, s]
  const m = (s: string): [ReactNode, string] => [<Mono>{s}</Mono>, s]
  if (c.kind === 'ssh') {
    return c.ssh
      ? [m(c.ssh.label), t(c.ssh.auth), m(`keymule ssh ${c.name} -- <command>`)]
      : [t('SSH login with no server set')]
  }
  if (c.kind === 'login') {
    const user = (c.auth || '').replace(/^basic \((.*)\)$/, '$1')
    return [t(c.service), m(c.url), t(user), t('filled by sign_in in the keymule browser')]
  }
  if (c.kind && c.kind !== 'http') return [t(c.service), t(USE_HINT[c.kind])]
  return [t(c.service), t((c.hosts || []).join(', ')), t(c.auth), ...(c.installed || []).map(i => t(`copied to ${i}`))]
}

function CredRow({ c }: { c: Credential }) {
  const open = store.use(s => isOpen(s, c.name))
  const items = useCommonItems(c.name, c.description)
  const sub = credSub(c).filter(([, text]) => text)
  return (
    <Row name={c.name}>
      <Ico name={c.kind === 'ssh' ? 'terminal' : c.kind === 'login' ? 'globe' : 'key'} />
      <span className="body">
        <RowTitle name={c.name} />
        <span className={open ? 'sub wrap' : 'sub'} title={open ? undefined : sub.map(([, text]) => text).join(' · ')}>
          <Join parts={sub.map(([node]) => node)} />
        </span>
        {open ? <Details name={c.name} text={c.description} /> : null}
      </span>
      <span className="acc">
        {c.missing_secret ? <Badge kind="bad">Missing</Badge> : c.file_edited ? <Badge kind="warn">Edited outside keymule</Badge> : <Badge kind="ok">Active</Badge>}
        <RevealButton name={c.name} />
        <MoreMenu name={c.name} items={items} />
      </span>
    </Row>
  )
}

export function CredList() {
  const list = store.use(s => s.list)
  const error = store.use(s => s.error)
  const first = useFirstRows(5)
  if (!list) return error ? <div className="empty t-bad">{error}</div> : null
  const creds = list.credentials
  if (!creds.length) return <div className="empty">No API credentials yet. Add one below, or ask your agent: it opens the same dialog.</div>
  return <Paged rows={creds} id="creds" base={first} render={c => <CredRow key={c.name} c={c} />} />
}

const URL_HINT: Record<string, string> = {
  github: 'https://api.github.com',
  gitlab: 'https://gitlab.com/api/v4',
  bamboo: 'https://bamboo.yourcompany.com',
  jenkins: 'https://jenkins.yourcompany.com',
  generic: 'https://api.example.com',
}

export function AddForm() {
  const busy = store.use(() => isBusy('add'))
  const [svc, setSvc] = useState('github')
  const [url, setUrl] = useState('')
  const [host, setHost] = useState('')
  const [user, setUser] = useState('')
  const [port, setPort] = useState('')
  const [auth, setAuth] = useState('password')
  const [keyFile, setKeyFile] = useState('')
  const ssh = svc === 'ssh'
  const submit = () => {
    if (ssh) {
      const body = { service: svc, host: host.trim(), username: user.trim(), port: port.trim(), auth, key_file: keyFile.trim() }
      if (body.auth === 'keyfile' && !body.key_file) { toast('Give the key file\'s path, like ~/.ssh/id_ed25519.', 'bad'); return }
      if (!body.host || !body.username) { toast('Give the server\'s host or IP and the username.', 'bad'); return }
      const msg = body.auth === 'keyfile' ? 'Saving…' : `A keymule dialog is open. Type the ${body.auth === 'key' ? 'private key' : 'password'} there.`
      run('add', msg, () => api('/api/add', body), () => { setHost(''); setUser(''); setPort(''); setKeyFile('') })
      return
    }
    run('add', 'A keymule dialog is open. Type the token there.', () => api('/api/add', { service: svc, url: url.trim() }), () => setUrl(''))
  }
  return (
    <>
      <form id="add" className="addline" autoComplete="off" onSubmit={e => { e.preventDefault(); submit() }}>
        <select id="svc" className="field" aria-label="Service" value={svc} onChange={e => setSvc(e.target.value)}>
          <option value="github">GitHub</option>
          <option value="gitlab">GitLab</option>
          <option value="bamboo">Bamboo</option>
          <option value="jenkins">Jenkins</option>
          <option value="stripe">Stripe</option>
          <option value="openai">OpenAI</option>
          <option value="generic">Any HTTP API</option>
          <option value="ssh">SSH login</option>
        </select>
        <input id="url" className="field url" type="url" spellCheck={false} aria-label="Base URL" hidden={ssh}
          placeholder={URL_HINT[svc] || ''} value={url} onChange={e => setUrl(e.target.value)} />
        <input id="sshhost" className="field url" type="text" spellCheck={false} placeholder="Host or IP, e.g. 203.0.113.7" aria-label="Host or IP" hidden={!ssh}
          value={host} onChange={e => setHost(e.target.value)} />
        <input id="sshuser" className="field short" type="text" spellCheck={false} placeholder="Username" aria-label="Username" hidden={!ssh}
          value={user} onChange={e => setUser(e.target.value)} />
        <input id="sshport" className="field port" type="text" inputMode="numeric" placeholder="22" aria-label="Port" hidden={!ssh}
          value={port} onChange={e => setPort(e.target.value)} />
        <select id="sshauth" className="field" aria-label="Log in with" hidden={!ssh} value={auth} onChange={e => setAuth(e.target.value)}>
          <option value="password">Password</option>
          <option value="key">Paste a private key</option>
          <option value="keyfile">Key file on this Mac</option>
        </select>
        <input id="sshkeyfile" className="field url" type="text" spellCheck={false} placeholder="~/.ssh/id_ed25519" aria-label="Key file path" hidden={!ssh || auth !== 'keyfile'}
          value={keyFile} onChange={e => setKeyFile(e.target.value)} />
        <button className="btn" type="submit" disabled={busy}>{ssh ? 'Add SSH login' : 'Add credential'}</button>
      </form>
      <p id="addnote" className="note">
        {ssh
          ? 'You type the password (or paste the key) into a keymule dialog, never into this page. Agents run commands with keymule ssh.'
          : 'You type the token into a keymule dialog, never into this page.'}
      </p>
    </>
  )
}
