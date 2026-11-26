// A tiny store: one state object, replaced (never mutated) on every change,
// and a hook that re-renders a component when the part it reads changes.
import { useSyncExternalStore } from 'react'

export function createStore<S extends object>(initial: S) {
  let state = initial
  const listeners = new Set<() => void>()
  const get = () => state
  const set = (patch: Partial<S> | ((s: S) => Partial<S>)) => {
    const p = typeof patch === 'function' ? patch(state) : patch
    state = { ...state, ...p }
    for (const l of listeners) l()
  }
  const subscribe = (l: () => void) => {
    listeners.add(l)
    return () => { listeners.delete(l) }
  }
  function use(): S
  function use<T>(pick: (s: S) => T): T
  function use<T>(pick?: (s: S) => T) {
    return useSyncExternalStore(subscribe, () => (pick ? pick(state) : state))
  }
  return { get, set, subscribe, use }
}
