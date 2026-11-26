// ---- drawing ----------------------------------------------------------------

function revealButton(name) {
  const busy = state.busy.has(`reveal:${name}`)
  return h('button', {
    class: 'btn ghost', type: 'button', disabled: busy, title: 'Show the value in a keymule window, never on this page',
    onclick: e => {
      e.stopPropagation()
      run(`reveal:${name}`, ap.pageMode ? `keymule asks here before showing "${name}".` : `Look for the keymule window: it asks before showing "${name}".`,
        () => api('/api/reveal', { name }))
    },
  }, busy ? 'Showing…' : 'Reveal')
}

const remove = name => run(`delete:${name}`, `A keymule window asks before deleting "${name}".`,
  () => api('/api/delete', { name }), loadPlan)

// Open a row and start editing its note.
function editNote(name) {
  state.filter = name
  state.describing = name
  render()
}

// The actions you need less often, behind one button. items: [label, fn, { danger, disabled, title }]
function moreMenu(name, items) {
  const open = state.menu === name
  const list = items.filter(Boolean)
  return h('span', { class: 'menuwrap' },
    h('button', {
      class: 'btn ghost iconly', type: 'button', title: 'More actions',
      'aria-label': `More actions for ${name}`, 'aria-haspopup': 'menu', 'aria-expanded': open ? 'true' : 'false',
      onclick: e => {
        e.stopPropagation()
        state.menu = open ? null : name
        render()
        if (!open) setTimeout(() => document.querySelector('.menu .mi:not(:disabled)')?.focus(), 0)
      },
      onkeydown: e => e.stopPropagation(),
    }, icon('more')),
    open ? h('div', {
      class: 'menu', role: 'menu',
      onclick: e => e.stopPropagation(),
      onkeydown: e => {
        e.stopPropagation()
        const mis = [...e.currentTarget.querySelectorAll('.mi:not(:disabled)')]
        const i = mis.indexOf(document.activeElement)
        if (e.key === 'ArrowDown') { e.preventDefault(); mis[(i + 1) % mis.length]?.focus() }
        if (e.key === 'ArrowUp') { e.preventDefault(); mis[(i - 1 + mis.length) % mis.length]?.focus() }
        if (e.key === 'Escape' || e.key === 'Tab') { state.menu = null; render() }
      },
    }, list.map(([label, fn, o = {}], i) => [
      o.danger && i ? h('div', { class: 'sep', role: 'separator' }) : null,
      h('button', {
        class: `mi${o.danger ? ' danger' : ''}`, type: 'button', role: 'menuitem', disabled: o.disabled, title: o.title,
        onclick: e => { e.stopPropagation(); state.menu = null; fn() },
      }, label),
    ])) : null)
}

// The menu everything shares: note, rotate, delete.
function commonItems(name, note) {
  const rotating = state.busy.has(`rotate:${name}`)
  const deleting = state.busy.has(`delete:${name}`)
  return [
    [note ? 'Edit note' : 'Add note', () => editNote(name), { title: 'Write what it is for, so you and your agents pick the right one' }],
    [rotating ? 'Rotating…' : 'Rotate', () => rotate(name), { disabled: rotating, title: 'Type a new value in a keymule dialog' }],
    [deleting ? 'Deleting…' : 'Delete', () => remove(name), { danger: true, disabled: deleting, title: 'Delete it from keymule (asks in a keymule window first)' }],
  ]
}

document.addEventListener('click', e => {
  if (state.menu && !e.target.closest('.menuwrap')) { state.menu = null; render() }
})

const isOpen = name => state.filter === name || state.describing === name

// Clicking a row opens it (details and note) and shows only its activity.
function rowAttrs(name) {
  const open = isOpen(name)
  return {
    class: `row click${open ? ' sel open' : ''}`,
    tabindex: '0',
    'aria-expanded': open ? 'true' : 'false',
    title: open ? 'Close' : 'Show details and only its activity',
    onclick: () => toggleRow(name),
    onkeydown: e => { if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); toggleRow(name) } },
  }
}

function toggleRow(name) {
  if (isOpen(name)) {
    state.filter = null
    if (state.describing === name) state.describing = null
  } else {
    state.filter = name
  }
  render()
}

const rowTitle = name => h('span', { class: 'title' }, h('span', { class: 'nm' }, name), h('span', { class: 'chev' }, icon('chev')))

// Shown when a row is open: the note in full, or a way to add one.
function details(name, text) {
  if (state.describing === name) return descLine(name, text)
  return h('span', { class: 'detail' },
    text ? h('span', { class: 'desc' }, text) : h('span', { class: 'desc none' }, 'No note yet.'),
    h('button', {
      class: 'btn notebtn', type: 'button',
      onclick: e => { e.stopPropagation(); editNote(name) },
      onkeydown: e => e.stopPropagation(),
    }, icon('pen'), text ? 'Edit note' : 'Add a note'))
}

// Long lists show their first `base` rows, then 20 more per click.
const STEP = 20
function paged(rows, key, base) {
  const extra = state.more[key] || 0
  const n = rows.length <= base + 1 ? rows.length : base + extra
  const out = rows.slice(0, n)
  const btn = (label, fn) => h('button', { class: 'more', type: 'button', onclick: () => { fn(); render() } }, label)
  const bar = [
    rows.length > n ? btn(`Show ${Math.min(STEP, rows.length - n)} more`, () => { state.more[key] = extra + STEP }) : null,
    extra ? btn('Show less', () => { state.more[key] = 0 }) : null,
  ].filter(Boolean)
  if (bar.length) out.push(h('div', { class: 'morebar' }, bar))
  return out
}
// The overview shows a few; a list's own page starts with 20.
const firstRows = n => (state.view === 'overview' ? n : STEP)

function descLine(name, text) {
  if (state.describing === name) {
    // Blur first: the page doesn't redraw a list while you type in it.
    const cancel = () => { input.blur(); state.describing = null; render() }
    const input = h('textarea', {
      class: 'field notefield', rows: 3, maxlength: 300, 'aria-label': `Description of ${name}`,
      placeholder: 'What it is for, e.g. Vultr VPS in Sydney (production)',
      onclick: e => e.stopPropagation(),
      onkeydown: e => {
        e.stopPropagation()
        if (e.key === 'Escape') cancel()
        // One line of text: Enter saves.
        if (e.key === 'Enter' && !e.shiftKey && !e.isComposing) { e.preventDefault(); save() }
      },
    })
    input.value = text || ''
    const save = () => {
      input.blur()
      state.describing = null
      run(`describe:${name}`, 'Saving…', () => api('/api/describe', { name, description: input.value.replace(/\s+/g, ' ').trim() }))
    }
    setTimeout(() => { input.focus(); input.setSelectionRange(input.value.length, input.value.length) }, 0)
    return h('form', {
      class: 'inline', onclick: e => e.stopPropagation(),
      onsubmit: e => { e.preventDefault(); save() },
    }, input,
    h('span', { class: 'notefoot' },
      h('button', { class: 'btn primary', type: 'submit' }, 'Save note'),
      h('button', { class: 'btn ghost', type: 'button', onclick: cancel }, 'Cancel'),
      h('span', { class: 'muted' }, 'Enter to save · Esc to cancel')))
  }
  return text ? h('span', { class: 'desc' }, text) : null
}

// How passwords, keys and AWS pairs are used: only by programs, never HTTP.
const USE_HINT = {
  password: 'for programs: keymule exec --env VAR',
  'private-key': 'for programs, as a file: keymule exec --file KEY',
  aws: 'for programs: keymule exec (AWS_ACCESS_KEY_ID, AWS_SECRET_ACCESS_KEY)',
}

function credSub(c) {
  if (c.kind === 'ssh') {
    return c.ssh
      ? [mono(c.ssh.label), c.ssh.auth, mono(`keymule ssh ${c.name} -- <command>`)]
      : ['SSH login with no server set']
  }
  if (c.kind === 'login') {
    const user = (c.auth || '').replace(/^basic \((.*)\)$/, '$1')
    return [c.service, mono(c.url), user, 'filled by sign_in in the keymule browser']
  }
  if (c.kind && c.kind !== 'http') return [c.service, USE_HINT[c.kind]]
  return [c.service, (c.hosts || []).join(', '), c.auth, ...(c.installed || []).map(i => `copied to ${i}`)]
}

function renderCreds(creds) {
  $('ncreds').textContent = creds.length || ''
  if (!creds.length) return [h('div', { class: 'empty' }, 'No API credentials yet. Add one below, or ask your agent: it opens the same dialog.')]
  return paged(creds.map(c => {
    const status = c.missing_secret ? badge('Missing', 'bad') : c.file_edited ? badge('Edited outside keymule', 'warn') : badge('Active', 'ok')
    const open = isOpen(c.name)
    return h('div', rowAttrs(c.name),
      ico(c.kind === 'ssh' ? 'terminal' : c.kind === 'login' ? 'globe' : 'key'),
      h('span', { class: 'body' },
        rowTitle(c.name),
        h('span', { class: open ? 'sub wrap' : 'sub', title: open ? null : credSub(c).map(x => x.textContent ?? x).join(' · ') }, join(credSub(c))),
        open ? details(c.name, c.description) : null),
      h('span', { class: 'acc' }, status, revealButton(c.name), moreMenu(c.name, commonItems(c.name, c.description))))
  }), 'creds', firstRows(5))
}

function renderServers(servers) {
  $('nservers').textContent = servers.length || ''
  if (!servers.length) return [h('div', { class: 'empty' }, 'No MCP server secrets yet. Plain-text keys in your agent configs show up above, ready to move.')]
  return paged(servers.map(s => {
    const what = [...(s.env || []), ...(s.headers || []).map(x => `${x} header`)]
    if (s.arg_secrets) what.push(`${s.arg_secrets} in arguments`)
    const used = s.used_by.length
    const single = what.length === 1 && !s.arg_secrets
    const open = isOpen(s.name)
    const row = h('div', rowAttrs(s.name),
      ico('server'),
      h('span', { class: 'body' },
        rowTitle(s.name),
        h('span', { class: open ? 'sub wrap' : 'sub' }, join([mono(what.join(', ')), used ? `used by ${used} config entr${used === 1 ? 'y' : 'ies'}` : 'no MCP server runs it'])),
        open ? details(s.name, s.description) : null),
      h('span', { class: 'acc' }, used ? badge('Active', 'ok') : badge('Unused'), revealButton(s.name),
        moreMenu(s.name, [
          !used && single ? ['Move to Credentials', () => { state.converting = state.converting === s.name ? null : s.name; render() },
            { title: 'Not an MCP server secret? Make it an SSH login or a password, without retyping it' }] : null,
          ...commonItems(s.name, s.description),
        ])))
    return state.converting === s.name ? h('div', { class: 'rowgroup' }, row, convertForm(s)) : row
  }), 'servers', firstRows(5))
}

// Move an unused MCP server secret to Credentials as an SSH login or a
// password. The value never comes here: keymule moves it, after a yes in its
// own window.
function convertForm(s) {
  const f = state.convertForm ||= { to: 'ssh', host: '', username: '', port: '', description: s.description || '' }
  const field = (key, attrs) => h('input', {
    class: 'field', type: 'text', value: f[key], spellcheck: 'false', ...attrs,
    oninput: e => { f[key] = e.target.value },
  })
  const busy = state.busy.has(`convert:${s.name}`)
  const ssh = f.to === 'ssh'
  const to = h('select', { class: 'field', 'aria-label': 'Move it as', onchange: e => { f.to = e.target.value; render() } },
    h('option', { value: 'ssh', selected: ssh }, 'SSH login'),
    h('option', { value: 'password', selected: !ssh }, 'Password for programs'))
  return h('form', {
    class: 'convert',
    onsubmit: e => {
      e.preventDefault()
      if (ssh && (!f.host.trim() || !f.username.trim())) { toast('Give the server\'s host or IP and the username.', 'bad'); return }
      const body = { name: s.name, to: f.to, host: f.host.trim(), username: f.username.trim(), port: f.port.trim(), description: f.description.trim() }
      run(`convert:${s.name}`, `A keymule window asks before moving "${s.name}".`, () => api('/api/convert', body),
        () => { state.converting = null; state.convertForm = null })
    },
  },
  h('p', { class: 'note' }, `Move "${s.name}" to Credentials. keymule moves the value itself: nobody sees it or types it again.`),
  h('div', { class: 'addline' }, to,
    ssh ? field('host', { class: 'field url', placeholder: 'Host or IP, e.g. 203.0.113.7', 'aria-label': 'Host or IP' }) : null,
    ssh ? field('username', { class: 'field short', placeholder: 'Username', 'aria-label': 'Username' }) : null,
    ssh ? field('port', { class: 'field port', placeholder: '22', inputmode: 'numeric', 'aria-label': 'Port' }) : null),
  h('div', { class: 'addline' },
    field('description', { class: 'field url', placeholder: 'What it is for (optional)', 'aria-label': 'Description', maxlength: 300 }),
    h('button', { class: 'btn', type: 'submit', disabled: busy }, busy ? 'Waiting…' : 'Move'),
    h('button', { class: 'btn ghost', type: 'button', onclick: () => { state.converting = null; state.convertForm = null; render() } }, 'Cancel')))
}

// The kinds of use, in chart order (bottom to top). Colors: --s-<kind>.
const KINDS = [
  ['api', 'API call', 'API calls'],
  ['command', 'command', 'commands'],
  ['mcp', 'MCP server start', 'MCP server starts'],
  ['reveal', 'reveal', 'reveals'],
]
const KIND_TITLE = { api: 'API calls', command: 'Commands', mcp: 'MCP servers', reveal: 'Reveals' }
const plural = (n, one, many) => `${n.toLocaleString()} ${n === 1 ? one : many}`
const swatch = k => h('i', { class: `sw s-${k}`, 'aria-hidden': 'true' })

function renderStats() {
  const list = state.list
  if (!list) return []
  const t = state.stats?.daily?.at(-1)
  const n = list.credentials.length + list.servers.length
  const stat = (k, v, cls, sub) => h('div', { class: 'stat' }, h('span', { class: 'k' }, k), h('span', { class: `v ${cls || ''}` }, v), sub || null)
  const parts = t ? KINDS.filter(([k]) => t[k]).map(([k, one, many]) => h('span', { class: 'part' }, swatch(k), plural(t[k], one, many))) : []
  const sub = h('span', { class: 'statsub' }, parts.length ? join(parts) : 'Nothing yet today',
    h('button', { class: 'btn link', type: 'button', onclick: () => { go('usage'); render() } }, 'Usage'))
  return [
    stat('Secrets stored', n),
    stat('Uses today', t ? t.total.toLocaleString() : '—', '', sub),
    stat('Failed today', t ? t.failed : 0, t?.failed ? 'bad' : ''),
  ]
}

async function loadStats() {
  try {
    state.stats = await api(`/api/stats?days=${state.urange}&tz=${-new Date().getTimezoneOffset()}`)
    render()
  } catch {}
}

function setRange(n) {
  state.urange = n
  try { sessionStorage.setItem('keymule-urange', String(n)) } catch {}
  loadStats()
}

