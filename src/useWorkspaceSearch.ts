import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { DateBounds } from './mediaDates'
import type { FileEntry, DirectoryListing } from './mediaModel'
const noDates: DateBounds = { field: 'modified', from: null, to: null, error: null }

type Page = { entries: FileEntry[]; total: number; totalVideos: number; page: number }
type Scan = { id: number; root: string; status: string; folders: number; videos: number; warnings: number; message: string | null; currentFolder: string }
const empty: FileEntry[] = []

type SearchOptions = { root: string | null; enabled: boolean; tagIds: number[]; matchAll: boolean; sort: string; descending: boolean; revision: number; listing: DirectoryListing | null; showVideos?: boolean; showImages?: boolean; dates?: DateBounds }
export function useWorkspaceSearch({ root, enabled, tagIds, matchAll, sort, descending, revision, listing, showVideos = true, showImages = true, dates = noDates }: SearchOptions) {
  const [scan, setScan] = useState<Scan | null>(null)
  const [restart, setRestart] = useState(0)
  const [scanError, setScanError] = useState<{ root: string; message: string } | null>(null)
  const [result, setResult] = useState<{ key: string; listing: object | null; data: Page | null; error: string | null } | null>(null)
  const filterKey = JSON.stringify([root, tagIds, matchAll, sort, descending, showVideos, showImages, dates.field, dates.from, dates.to])
  const [pageState, setPageState] = useState({ key: filterKey, page: 0 })
  if (pageState.key !== filterKey) setPageState({ key: filterKey, page: 0 })
  const page = pageState.key === filterKey ? pageState.page : 0
  const currentScan = scan?.root === root ? scan : null
  // Refresh the catalog results when discovery ends. Polling progress never interrupts playback.
  const scanVersion = currentScan && currentScan.status !== 'running' ? `${currentScan.id}:${currentScan.status}` : ''
  const key = JSON.stringify([filterKey, page, revision, scanVersion, enabled])
  const ready = result?.key === key && result.listing === listing

  useEffect(() => {
    if (!enabled || !root) return
    let alive = true
    let id: number | null = null
    let timer: ReturnType<typeof setTimeout> | undefined
    async function poll() {
      try {
        const status = await invoke<Scan>('workspace_scan_status', { scanId: id })
        if (!alive) return
        setScan(status)
        if (status.status === 'running') timer = setTimeout(() => void poll(), 500)
      } catch (cause) { if (alive) setScanError({ root: root!, message: String(cause) }) }
    }
    void invoke<Scan>('start_workspace_scan', { expectedRoot: root }).then(status => {
      id = status.id
      if (!alive) { void invoke('cancel_workspace_scan', { scanId: id }).catch(() => {}); return }
      setScan(status); setScanError(null)
      if (status.status === 'running') timer = setTimeout(() => void poll(), 500)
    }).catch(cause => { if (alive) setScanError({ root, message: String(cause) }) })
    return () => { alive = false; clearTimeout(timer); if (id !== null) void invoke('cancel_workspace_scan', { scanId: id }).catch(() => {}) }
  }, [root, enabled, restart])

  useEffect(() => {
    if (!enabled || !root) return
    let alive = true
    void invoke<Page>('search_workspace', { query: { expectedRoot: root, tagIds, matchAll, sort, descending, page, showVideos, showImages, dateRange: { field: dates.field, from: dates.from, to: dates.to } } })
      .then(data => { if (alive) setResult({ key, listing, data, error: null }) })
      .catch(cause => { if (alive) setResult({ key, listing, data: null, error: String(cause) }) })
    return () => { alive = false }
  }, [root, enabled, tagIds, matchAll, sort, descending, page, key, listing, showVideos, showImages, dates.field, dates.from, dates.to])

  return {
    entries: ready ? result?.data?.entries ?? empty : empty,
    thumbnailRevision: ready ? result?.data ?? empty : empty,
    total: ready ? result?.data?.total ?? 0 : 0,
    totalVideos: ready ? result?.data?.totalVideos ?? 0 : 0,
    page: ready ? result?.data?.page ?? page : page,
    ready, scan: currentScan, scanError: scanError?.root === root ? scanError.message : null,
    error: ready ? result?.error : null,
    setPage: (page: number) => setPageState({ key: filterKey, page }),
    rescan: () => setRestart(value => value + 1),
    cancel: () => { if (currentScan) void invoke('cancel_workspace_scan', { scanId: currentScan.id }).catch(cause => setScanError({ root: root!, message: String(cause) })) },
  }
}
