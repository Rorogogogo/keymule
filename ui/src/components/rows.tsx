// What the lists share: clickable rows, the ⋯ menu, notes, and paging.
import { useEffect, useRef, useState, type ReactNode } from 'react'
import {
  editNote, isBusy, isOpen, remove, reveal, rotate, saveNote, setMenu, showMore, store, toggleRow,
} from '../lib/app'
import { Icon } from './ui'

// Clicking a row opens it (details and note) and shows only its activity.
export function Row({ name, children }: { name: string; children: ReactNode }) {
  const open = store.use(s => isOpen(s, name))
  return (
    <div
      className={`row click${open ? ' sel open' : ''}`}
      tabIndex={0}
      aria-expanded={open}
      title={open ? 'Close' : 'Show details and only its activity'}
      onClick={() => toggleRow(name)}
      onKeyDown={e => {
        if (e.target === e.currentTarget && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); toggleRow(name) }
      }}
    >
      {children}
    </div>
  )
}

export const RowTitle = ({ name }: { name: string }) => (
  <span className="title"><span className="nm">{name}</span><span className="chev"><Icon name="chev" /></span></span>
)

export function RevealButton({ name }: { name: string }) {
  const busy = store.use(() => isBusy(`reveal:${name}`))
  return (
    <button
      className="btn ghost" type="button" disabled={busy} title="Show the value in a keymule window, never on this page"
      onClick={e => { e.stopPropagation(); reveal(name) }}
    >
      {busy ? 'Showing…' : 'Reveal'}
    </button>
  )
}

export type MenuItem = [label: string, fn: () => void, opts?: { danger?: boolean; disabled?: boolean; title?: string }] | null

// The actions you need less often, behind one button.
export function MoreMenu({ name, items }: { name: string; items: MenuItem[] }) {
  const open = store.use(s => s.menu === name)
  const menu = useRef<HTMLDivElement>(null)
  const list = items.filter(Boolean) as Exclude<MenuItem, null>[]
  useEffect(() => {
    if (open) menu.current?.querySelector<HTMLButtonElement>('.mi:not(:disabled)')?.focus()
  }, [open])
  return (
    <span className="menuwrap">
      <button
        className="btn ghost iconly" type="button" title="More actions"
        aria-label={`More actions for ${name}`} aria-haspopup="menu" aria-expanded={open}
        onClick={e => { e.stopPropagation(); setMenu(open ? null : name) }}
        onKeyDown={e => e.stopPropagation()}
      >
        <Icon name="more" />
      </button>
      {open ? (
        <div
          ref={menu} className="menu" role="menu"
          onClick={e => e.stopPropagation()}
          onKeyDown={e => {
            e.stopPropagation()
            const mis = [...e.currentTarget.querySelectorAll<HTMLButtonElement>('.mi:not(:disabled)')]
            const i = mis.indexOf(document.activeElement as HTMLButtonElement)
            if (e.key === 'ArrowDown') { e.preventDefault(); mis[(i + 1) % mis.length]?.focus() }
            if (e.key === 'ArrowUp') { e.preventDefault(); mis[(i - 1 + mis.length) % mis.length]?.focus() }
            if (e.key === 'Escape' || e.key === 'Tab') setMenu(null)
          }}
        >
          {list.map(([label, fn, o = {}], i) => [
            o.danger && i ? <div key={`sep${i}`} className="sep" role="separator" /> : null,
            <button
              key={label} className={`mi${o.danger ? ' danger' : ''}`} type="button" role="menuitem"
              disabled={o.disabled} title={o.title}
              onClick={e => { e.stopPropagation(); setMenu(null); fn() }}
            >
              {label}
            </button>,
          ])}
        </div>
      ) : null}
    </span>
  )
}

// The menu everything shares: note, rotate, delete.
export function useCommonItems(name: string, note: string): MenuItem[] {
  const rotating = store.use(() => isBusy(`rotate:${name}`))
  const deleting = store.use(() => isBusy(`delete:${name}`))
  return [
    [note ? 'Edit note' : 'Add note', () => editNote(name), { title: 'Write what it is for, so you and your agents pick the right one' }],
    [rotating ? 'Rotating…' : 'Rotate', () => rotate(name), { disabled: rotating, title: 'Type a new value in a keymule dialog' }],
    [deleting ? 'Deleting…' : 'Delete', () => remove(name), { danger: true, disabled: deleting, title: 'Delete it from keymule (asks in a keymule window first)' }],
  ]
}

// Shown when a row is open: the note in full, or a way to add one.
export function Details({ name, text }: { name: string; text: string }) {
  const editing = store.use(s => s.describing === name)
  if (editing) return <NoteEditor name={name} text={text} />
  return (
    <span className="detail">
      {text ? <span className="desc">{text}</span> : <span className="desc none">No note yet.</span>}
      <button
        className="btn notebtn" type="button"
        onClick={e => { e.stopPropagation(); editNote(name) }}
        onKeyDown={e => e.stopPropagation()}
      >
        <Icon name="pen" />{text ? 'Edit note' : 'Add a note'}
      </button>
    </span>
  )
}

function NoteEditor({ name, text }: { name: string; text: string }) {
  const [value, setValue] = useState(text || '')
  const input = useRef<HTMLTextAreaElement>(null)
  useEffect(() => {
    const el = input.current
    if (el) { el.focus(); el.setSelectionRange(el.value.length, el.value.length) }
  }, [])
  const cancel = () => store.set({ describing: null })
  const save = () => saveNote(name, value)
  return (
    <form className="inline" onClick={e => e.stopPropagation()} onSubmit={e => { e.preventDefault(); save() }}>
      <textarea
        ref={input} className="field notefield" rows={3} maxLength={300} aria-label={`Description of ${name}`}
        placeholder="What it is for, e.g. Vultr VPS in Sydney (production)"
        value={value} onChange={e => setValue(e.target.value)}
        onClick={e => e.stopPropagation()}
        onKeyDown={e => {
          e.stopPropagation()
          if (e.key === 'Escape') cancel()
          // One line of text: Enter saves.
          if (e.key === 'Enter' && !e.shiftKey && !e.nativeEvent.isComposing) { e.preventDefault(); save() }
        }}
      />
      <span className="notefoot">
        <button className="btn primary" type="submit">Save note</button>
        <button className="btn ghost" type="button" onClick={cancel}>Cancel</button>
        <span className="muted">Enter to save · Esc to cancel</span>
      </span>
    </form>
  )
}

// Long lists show their first `base` rows, then 20 more per click.
export const STEP = 20
export function Paged<T>({ rows, id, base, render }: { rows: T[]; id: string; base: number; render: (row: T, i: number) => ReactNode }) {
  const extra = store.use(s => s.more[id] || 0)
  const n = rows.length <= base + 1 ? rows.length : base + extra
  return (
    <>
      {rows.slice(0, n).map(render)}
      {rows.length > n || extra ? (
        <div className="morebar">
          {rows.length > n ? <button className="more" type="button" onClick={() => showMore(id, extra + STEP)}>Show {Math.min(STEP, rows.length - n)} more</button> : null}
          {extra ? <button className="more" type="button" onClick={() => showMore(id, 0)}>Show less</button> : null}
        </div>
      ) : null}
    </>
  )
}

// The overview shows a few; a list's own page starts with 20.
export const useFirstRows = (n: number) => store.use(s => (s.view === 'overview' ? n : STEP))
