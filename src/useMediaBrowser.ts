import { restoreRatingFilter } from './mediaRatings'
import type { RatingFilterState } from './mediaRatings'
import { useEffect, useMemo, useRef, useState } from 'react'
import { getPreference, setPreference } from './preferences'
import { dateBounds, emptyDateFilter, restoreDateFilter } from './mediaDates'
import type { DateFilterState } from './mediaDates'
import { useLocalDay } from './useLocalDay'
import { useTagFilter } from './useTagFilter'
import { useWorkspaceSearch } from './useWorkspaceSearch'
import { folderResults } from './mediaResults'
import { filterMedia, shownFilePaths, visibleSelection } from './mediaFilter'
import type { DirectoryListing, FileEntry } from './mediaModel'
import type { MediaSort, SortDirection } from './mediaSort'
import { nextSelection } from './mediaSelection'

export function useMediaBrowser({ root, listing, revision, view, sort, direction }: { root: string | null; listing: DirectoryListing | null; revision: number; view: string; sort: MediaSort; direction: SortDirection }) {
  const [showVideos, setShowVideos] = useState(() => getPreference('framewise.showVideos') !== 'false')
  const [showImages, setShowImages] = useState(() => getPreference('framewise.showImages') !== 'false')
  const [rating, setRatingState] = useState(() => restoreRatingFilter(getPreference('framewise.ratingFilter')))
  const [dateScope, setDateScope] = useState(() => restoreDateFilter(getPreference('framewise.dateFilter')))
  const [searchScope, setSearchScope] = useState({ root, workspace: false })
  if (root !== null && dateScope.root !== root) setDateScope({ root, value: emptyDateFilter })
  if (searchScope.root !== root) setSearchScope({ root, workspace: false })
  useEffect(() => {
    if (root !== null && dateScope.root === root) setPreference('framewise.dateFilter', JSON.stringify(dateScope))
  }, [root, dateScope])
  const localDay = useLocalDay()
  const dates = useMemo(() => dateBounds(dateScope.value, localDay), [dateScope.value, localDay])
  const workspace = searchScope.root === root && searchScope.workspace
  const tagFilter = useTagFilter(listing, root, revision, view, !workspace)
  const workspaceSearch = useWorkspaceSearch({ root, enabled: workspace && view === 'media', tagIds: tagFilter.ids, matchAll: tagFilter.matchAll, sort, descending: direction === 'desc', revision, listing, showVideos, showImages, dates, rating })
  const resultFilters = useMemo(() => ({ showVideos, showImages, dates, rating, tagActive: tagFilter.active, matchingIds: tagFilter.matches, sort, direction }), [showVideos, showImages, dates, rating, tagFilter.active, tagFilter.matches, sort, direction])
  const sortedMediaEntries = useMemo(() => folderResults(listing?.entries ?? [], { showVideos, showImages, dates, rating, sort, direction, tagActive: false, matchingIds: [] }), [listing, showVideos, showImages, dates, rating, sort, direction])
  const mediaEntries = useMemo(() => workspace ? workspaceSearch.entries : filterMedia(sortedMediaEntries, tagFilter.active, tagFilter.matches), [workspace, workspaceSearch.entries, sortedMediaEntries, tagFilter.active, tagFilter.matches])
  return {
    rating, showVideos, showImages, dateScope, dates, workspace, tagFilter, workspaceSearch, resultFilters, sortedMediaEntries, mediaEntries,
    resultsReady: workspace ? workspaceSearch.ready : tagFilter.ready,
    scanRunning: workspace && workspaceSearch.scan?.status === 'running',
    setRating: (value: RatingFilterState) => { setRatingState(value); setPreference('framewise.ratingFilter', JSON.stringify(value)) },
    setWorkspace: (workspace: boolean) => setSearchScope({ root, workspace }),
    setDates: (value: DateFilterState) => setDateScope({ root, value }),
    setTypes: (videos: boolean, images: boolean) => {
      setShowVideos(videos); setShowImages(images)
      setPreference('framewise.showVideos', String(videos)); setPreference('framewise.showImages', String(images))
    },
  }
}

export function useMediaSelection(entries: readonly FileEntry[], ready: boolean) {
  const [selectedPaths, setSelectedPaths] = useState<string[]>([])
  const selectionAnchor = useRef<string | null>(null)
  return {
    selectedPaths, setSelectedPaths, selectionAnchor,
    selectEntry: (entry: FileEntry, index: number, modifiers: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) => {
      const update = nextSelection(entries, selectedPaths, selectionAnchor.current, entry, index, modifiers)
      if (!update.openFolder) { setSelectedPaths(update.paths); selectionAnchor.current = update.anchor }
      return update
    },
    selectAll: (current: FileEntry | null) => {
      if (!ready) return undefined
      const paths = shownFilePaths(entries)
      if (!paths.length) return undefined
      setSelectedPaths(paths); selectionAnchor.current = paths[0]
      return current && paths.includes(current.path) ? current : entries.find(entry => entry.path === paths[0])
    },
    reconcile: () => {
      if (!ready) return
      const kept = visibleSelection(selectedPaths, entries)
      if (kept.length !== selectedPaths.length) setSelectedPaths(kept)
      if (selectionAnchor.current && !entries.some(entry => entry.path === selectionAnchor.current)) selectionAnchor.current = null
    },
  }
}
