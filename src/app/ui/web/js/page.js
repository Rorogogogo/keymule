// A newer release, or an upgrade this page's keymule hasn't restarted onto.
// Quiet on purpose: everything still works. keymule never updates itself.
function renderUpdate() {
  const u = state.update
  if (!u) return []
  const copyCmd = cmd => h('button', {
    class: 'btn', type: 'button',
    onclick: async e => {
      try { await navigator.clipboard.writeText(cmd); e.target.textContent = 'Copied' } catch { e.target.textContent = 'Couldn’t copy' }
    },
  }, 'Copy')
  if (u.replaced) {
    const menubar = state.host === 'menubar'
    return [h('div', { class: 'pairing slim update' },
      h('div', { class: 'pairtext' },
        h('strong', null, 'keymule was updated. Restart it to use the new version'),
        h('span', null, menubar
          ? 'Restart your agents too (Claude Code, Codex, Cursor), so the keymule they started is the new one.'
          : 'Stop `keymule ui` and start it again. Restart your agents too, so the keymule they started is the new one.')),
      menubar ? h('div', { class: 'pairgo' }, h('button', {
        class: 'btn primary', type: 'button',
        onclick: async e => {
          e.target.disabled = true; e.target.textContent = 'Restarting…'
          try { await api('/api/restart', {}) } catch {}
        },
      }, 'Restart now')) : null)]
  }
  if (!u.latest) return []
  return [h('div', { class: 'pairing slim update' },
    h('div', { class: 'pairtext' },
      h('strong', null, `keymule ${u.latest} is out`),
      h('span', null, `You have ${u.current}. `, h('a', { href: u.notes, target: '_blank', rel: 'noopener' }, 'What’s new'), '. To update, run:'),
      h('code', { class: 'updcmd' }, u.command)),
    h('div', { class: 'pairgo' }, copyCmd(u.command)))]
}

function renderBanner() {
  // No keychain and no passphrase: nothing can be saved, so say how to fix it first.
  const vp = state.list?.vault_problem
  if (vp) {
    const [why, ...help] = vp.split('\n\n')
    return [h('div', { class: 'callout' },
      ico('warn'),
      h('div', { class: 'body' },
        h('span', { class: 'title' }, 'keymule can’t store secrets on this machine yet'),
        h('span', { class: 'sub' }, `${why}. Set a passphrase where your agents run, then restart them:`),
        h('pre', { class: 'plan' }, help.join('\n\n'))))]
  }
  const p = state.plan
  if (!p || !p.has) return []
  const busy = state.busy.has('adopt')
  return [h('div', { class: 'callout' },
    ico(busy ? 'spin' : 'warn'),
    h('div', { class: 'body' },
      h('span', { class: 'title' }, p.summary),
      h('span', { class: 'sub' }, `${p.names.join(', ')}: keymule can move them into the keychain, and the servers keep working.`),
      h('div', { class: 'acts' },
        h('button', { class: 'btn primary', type: 'button', disabled: busy, onclick: applyPlan }, state.confirmApply ? 'Click again to confirm' : 'Move to keychain'),
        h('button', { class: 'btn', type: 'button', onclick: () => { state.planOpen = !state.planOpen; render() } }, state.planOpen ? 'Hide details' : 'Show details')),
      state.planOpen ? h('pre', { class: 'plan' }, p.text) : null))]
}

const SEVERITY = { leaked: 0, plaintext: 1, readable: 2 }
const SEV_LABEL = { leaked: 'sent to a model', plaintext: 'plain text in a config', readable: 'readable by agents' }

function renderLeaks() {
  const d = state.doctor
  $('scan').textContent = d === 'running' ? 'Scanning…' : d ? 'Scan again' : 'Scan'
  $('scan').disabled = d === 'running'
  if (d === null) return [h('div', { class: 'scanbox' }, ico('shield'), h('span', {}, 'Look for keys that leaked into agent transcripts, sit in plain text in configs, or are readable in .env files and shell history. Values stay masked.'))]
  if (d === 'running') return [scanBox()]
  if (d.error) return [h('div', { class: 'scanbox' }, ico('x'), h('span', { class: 't-bad' }, d.error))]

  const worst = e => e.locations.map(l => l.severity).sort((a, b) => SEVERITY[a] - SEVERITY[b])[0]
  const entries = [...d.entries].sort((a, b) => SEVERITY[worst(a)] - SEVERITY[worst(b)])
  const count = sev => entries.filter(e => worst(e) === sev).length
  const stored = entries.filter(e => e.stored_as).length
  const st = d.stats || {}
  const c = (k, v, cls) => h('div', {}, h('span', { class: 'k' }, k), h('span', { class: `v ${v ? cls : ''}` }, v))
  const counts = h('div', { class: 'counts' },
    c('Leaked', count('leaked'), 't-bad'), c('Plain text', count('plaintext'), 't-warn'), c('Readable', count('readable'), ''), c('In keymule', stored, ''))
  if (!entries.length) {
    return [counts, h('div', { class: 'empty' }, `Nothing found in ${st.transcripts ?? 0} transcripts, ${(st.configs || []).length} configs and ${st.env_files ?? 0} .env files.`)]
  }
  const rows = paged(entries, 'leaks', 10).map(e => {
    if (e instanceof Node) return e
    const sev = worst(e)
    const first = e.locations[0] || {}
    const more = e.locations.length > 1 ? `+${e.locations.length - 1} more` : null
    const b = e.stored_as ? badge('In keymule', 'ok') : sev === 'leaked' ? badge('Rotate it', 'bad') : sev === 'plaintext' ? badge('Move it', 'warn') : badge('Readable')
    return h('div', { class: 'row', title: e.locations.map(l => `${l.place}${l.date ? ' · ' + l.date : ''}`).join('\n') },
      ico(sev === 'leaked' ? 'warn' : 'eye'),
      h('span', { class: 'body' },
        h('span', { class: 'title' }, mono(e.masked)),
        h('span', { class: 'sub' }, join([e.label, SEV_LABEL[sev], first.place, first.date, more]))),
      h('span', { class: 'acc' }, b))
  })
  return [counts, ...rows]
}

// Audit lines that aren't HTTP requests: what to call them.
const EVENTS = {
  START: 'Started', EXEC: 'Ran', INSTALL: 'Copied to git', UNINSTALL: 'Removed from git',
  'GH-SECRET': 'Copied to GitHub', 'AUTO-ALLOW': 'Allowed', 'ALLOW-ALL-ON': 'Allow all on',
  'ALLOW-ALL-OFF': 'Allow all off', PAIR: 'Paired', 'BAD-APPROVAL': 'Refused',
  'PAGE-LOGIN': 'Signed in', 'ANSWERED-IN-PAGE': 'Answered', 'DENIED-IN-PAGE': 'Denied',
  SSH: 'Logged in', CONVERT: 'Moved', RENAME: 'Renamed',
}
// keymule's own events have no credential: show them as keymule's.
const actName = a => (a.name === '-' ? 'keymule' : a.name)

function renderActivity() {
  const f = state.filter
  $('filter').replaceChildren(f ? h('button', { class: 'chip', type: 'button', onclick: () => toggleFilter(f), title: 'Show everything' }, f, ' ×') : '')
  const all = state.audit.filter(a => !f || a.name === f).slice(-1000).reverse()
  if (!all.length) return [h('div', { class: 'empty' }, f ? `Nothing from ${f} yet.` : 'Requests and server starts show up here as they happen.')]
  return paged(all, 'act', state.view === 'activity' ? 50 : 8).map(a => {
    if (a instanceof Node) return a
    const event = EVENTS[a.method]
    const shown = a.method === 'REVEAL'
    const ok = a.status >= 200 && a.status < 300
    return h('div', { class: 'act' },
      h('span', { class: 'name', onclick: () => toggleFilter(a.name), title: 'Show only its activity' }, actName(a)),
      h('span', { class: 'req', title: a.url }, h('span', { class: 'm' }, a.method), h('span', { class: 'u' }, shown ? 'shown to you in a keymule window' : a.url)),
      h('span', { class: 'st' }, event ? badge(event, a.method === 'BAD-APPROVAL' ? 'bad' : '') : shown ? badge('Revealed', 'warn') : badge(a.status || '—', ok ? 'ok' : 'bad')),
      h('span', { class: 'when' }, when(a.ts)))
  })
}

function renderNav() {
  const list = state.list
  const d = state.doctor
  const leaks = d && d !== 'running' && d.entries ? d.entries.filter(e => !e.stored_as).length : null
  const counts = { credentials: list?.credentials.length, servers: list?.servers.length, leaks }
  $('nav').replaceChildren(...VIEWS.map(([id, label, ic]) => h('button', {
    type: 'button', class: state.view === id ? 'on' : null, 'aria-current': state.view === id ? 'page' : null,
    onclick: () => go(id), title: collapsed ? label : null,
  }, icon(ic), h('span', { class: 'label' }, label), counts[id] != null ? h('span', { class: `n ${id === 'leaks' && counts[id] ? 'bad' : ''}` }, counts[id]) : null)))
  const v = VIEWS.find(x => x[0] === state.view) || VIEWS[0]
  $('ptitle').textContent = v[1]
  $('psub').textContent = v[3]
  $('main').className = `main v-${v[0]}`
  for (const el of document.querySelectorAll('[data-in]')) el.hidden = !el.dataset.in.split(' ').includes(v[0])
}

const drawn = {}
const objId = new WeakMap()
let nextId = 1
const idOf = o => (o && typeof o === 'object' ? (objId.get(o) || (objId.set(o, nextId++), objId.get(o))) : o)
function draw(id, key, build) {
  const k = JSON.stringify(key)
  if (drawn[id] === k) return
  drawn[id] = k
  $(id).replaceChildren(...build())
}

function render() {
  const y = window.scrollY
  renderNav()
  const live = $('live')
  live.className = `live${state.live === false ? ' off' : ''}`
  if (state.live === false) {
    live.replaceChildren(h('span', { class: 'dot' }), state.forbidden
      ? 'No access: open the link `keymule ui` printed'
      : 'Disconnected: run `keymule ui` again')
  } else if (state.live) {
    live.replaceChildren(h('span', { class: 'dot' }), 'Live')
  }
  $('version').textContent = state.version ? `v${state.version}` : ''
  const list = state.list
  $('stats').replaceChildren(...renderStats())
  $('banner').replaceChildren(...renderBanner())
  draw('update', [state.update, state.host], renderUpdate)
  const typing = el => el.contains(document.activeElement) && document.activeElement.matches('input, select, textarea, .mi')
  const busy = [...state.busy]
  const rowState = [state.view, state.filter, state.describing, state.menu, busy]
  if (list) {
    if (!typing($('creds'))) draw('creds', [list.credentials, state.more.creds, ...rowState], () => renderCreds(list.credentials))
    if (!typing($('servers'))) draw('servers', [list.servers, state.more.servers, state.converting, state.convertForm?.to, ...rowState], () => renderServers(list.servers))
  } else if (state.error) {
    draw('creds', ['error', state.error], () => [h('div', { class: 'empty t-bad' }, state.error)])
  }
  $('add').querySelector('button').disabled = state.busy.has('add')
  draw('leaks', [idOf(state.doctor), state.more.leaks, state.doctor === 'running' ? state.scan?.id : null], renderLeaks)
  if (state.doctor === 'running') updateScanBox()
  const last = state.audit[state.audit.length - 1]
  draw('activity', [state.audit.length, last?.ts, last?.name, state.filter, state.more.act, state.view, new Date().toDateString()], renderActivity)
  if (state.view === 'usage') draw('usage', [idOf(state.stats), state.utable, state.list?.credentials.length, state.list?.servers.length, Math.round($('usage').clientWidth / 40)], renderUsage)
  // Nothing above should move the page; put it back if the browser did.
  if (Math.abs(window.scrollY - y) > 1) window.scrollTo(0, y)
  updateProgress()
}

