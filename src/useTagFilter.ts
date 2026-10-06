import { useEffect, useMemo, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'


type Tag = { id: number; name: string }
type Scope = { root: string | null; ids: number[]; matchAll: boolean }
const emptyIds: number[] = []
export function useTagFilter(listing: { path: string } | null, root: string | null, revision: number, view: string) {
  const [scope, setScope] = useState<Scope>({ root: null, ids: [], matchAll: true })
  // Reset on every workspace change, including returning to an earlier workspace.
  if (scope.root !== root) setScope({ root, ids: [], matchAll: true })
  const [tags, setTags] = useState<Tag[]>([])
  const [catalogError, setCatalogError] = useState<string | null>(null)
  const [result, setResult] = useState<{ key: string; listing: object; ids: number[]; error: string | null } | null>(null)
  const idsKey = JSON.stringify(scope.root === root ? scope.ids : [])
  const ids = useMemo(() => JSON.parse(idsKey) as number[], [idsKey])
  const matchAll = scope.root === root ? scope.matchAll : true
  const key = JSON.stringify([root, listing?.path, ids, matchAll, revision])
  const active = ids.length > 0
  const ready = !active || (result?.key === key && result.listing === listing)
  const matches = ready ? result?.ids ?? emptyIds : emptyIds

  useEffect(() => {
    if (!listing || view !== 'media') return
    let current = true
    void invoke<Tag[]>('list_tags', { query: '' }).then(available => {
      if (!current) return
      setTags(available); setCatalogError(null)
      const existing = new Set(available.map(tag => tag.id))
      setScope(previous => {
        if (previous.root !== root) return previous
        const kept = previous.ids.filter(id => existing.has(id))
        return kept.length === previous.ids.length ? previous : { ...previous, ids: kept }
      })
    }).catch(cause => { if (current) setCatalogError(String(cause)) })
    return () => { current = false }
  }, [listing, root, revision, view])
  useEffect(() => {
    if (!active || !listing || view !== 'media') return
    let current = true
    void invoke<number[]>('filter_folder_videos', { path: listing.path, tagIds: ids, matchAll })
      .then(matches => { if (current) setResult({ key, listing, ids: matches, error: null }) })
      .catch(cause => { if (current) setResult({ key, listing, ids: [], error: String(cause) }) })
    return () => { current = false }
  }, [active, listing, view, key, ids, matchAll])

  return { tags, ids, matchAll, active, ready, matches, error: catalogError ?? (ready && active ? result?.error : null),
    change: (nextIds: number[], nextMode = matchAll) => setScope({ root, ids: nextIds, matchAll: nextMode }) }
}


