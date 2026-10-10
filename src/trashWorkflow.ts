import { allRatings, filterRatings, ratingFilterLabel } from './mediaRatings'
import type { RatingFilterState } from './mediaRatings'
import type { DirectoryListing, FileEntry, TrashBatchResult } from './mediaModel'
import type { DateBounds, DateFilterState } from './mediaDates'
import { filterMediaDates } from './mediaDates'
import { filterMediaTypes } from './mediaTypes'

type FilterSnapshot = { expectedRoot: string; date: { field: 'created' | 'modified'; from: number | null; to: number | null }; showVideos: boolean; showImages: boolean; rating?: RatingFilterState }
export type TrashRequest = { kind: 'folder' | 'files'; paths: string[]; filter: FilterSnapshot; title: string; message: string; name: string }
export function prepareTrash({ file, selectedPaths, entries, ready, root, dates, dateState, showVideos, showImages, rating = allRatings }: { file: FileEntry; selectedPaths: readonly string[]; entries: readonly FileEntry[]; ready: boolean; root: string | null; dates: DateBounds; dateState: DateFilterState; showVideos: boolean; showImages: boolean; rating?: RatingFilterState }): TrashRequest {
  if (!root) throw new Error('Choose a workspace first.')
  const paths = [...new Set(file.isDirectory ? [file.path] : selectedPaths.includes(file.path) ? selectedPaths : [file.path])]
  const matching = filterRatings(filterMediaDates(filterMediaTypes(entries, showVideos, showImages), dates), rating).filter(entry => !entry.isDirectory && paths.includes(entry.path))
  if (!file.isDirectory && (!ready || dates.error || !paths.length || matching.length !== paths.length)) throw new Error('The selection no longer matches the displayed results. Refresh and select the files again. No files were deleted.')
  const count = paths.length
  const selectedDates = matching.map(entry => dates.field === 'created' ? entry.createdAt : entry.modifiedAt).filter((value): value is number => value != null)
  const span = selectedDates.length ? `${new Date(selectedDates.reduce((a, b) => Math.min(a, b))).toLocaleDateString()} – ${new Date(selectedDates.reduce((a, b) => Math.max(a, b))).toLocaleDateString()}` : 'Unavailable'
  const range = dateState.preset === 'all' ? 'All dates' : dateState.preset === 'custom' ? `${dateState.from || 'Any'} to ${dateState.to || 'Any'}` : dateState.preset === '7' ? 'Last 7 days' : dateState.preset === '30' ? 'Last 30 days' : dateState.preset
  const summary = `\n\nDate filter: ${dates.field === 'created' ? 'Created' : 'Modified'} · ${range}.\nSelected file dates: ${span}${selectedDates.length < count ? ` (${count - selectedDates.length} unavailable)` : ''}.`
  return {
    kind: file.isDirectory ? 'folder' : 'files', paths, name: file.name,
    filter: { expectedRoot: root, date: { field: dates.field, from: dates.from, to: dates.to }, showVideos, showImages, rating: { ...rating } },
    title: file.isDirectory ? 'Move folder to Trash' : count === 1 ? 'Move file to Trash' : `Move ${count} files to Trash`,
    message: (file.isDirectory ? `Move folder “${file.name}” and everything inside it to Trash?` : count === 1 ? `Move “${file.name}” to Trash?` : `Move ${count} selected files to Trash?`) + ` You can restore ${file.isDirectory ? 'it' : 'them'} from your system's Trash or Recycle Bin.` + (file.isDirectory ? '' : summary + `\nRating filter: ${ratingFilterLabel(rating)}.`),
  }
}
export async function executeTrash(request: TrashRequest, services: { confirm: (request: TrashRequest) => Promise<boolean>; beforeMove: () => void; move: (request: TrashRequest) => Promise<TrashBatchResult> }) {
  if (!await services.confirm(request)) return { status: 'cancelled' as const }
  services.beforeMove()
  return { status: 'completed' as const, result: await services.move(request) }
}
export function trashMessage(request: TrashRequest, result: TrashBatchResult): string {
  if (result.error) return `Moved ${result.movedCount} of ${request.paths.length} files to Trash. ${result.error}`
  return request.kind === 'folder' || request.paths.length === 1 ? `Moved “${request.name}” to Trash.` : `Moved ${result.movedCount} files to Trash.`
}
export function folderTrashResult<T extends FileEntry>(entries: T[], selected: readonly string[], clickedPath: string, oldEntries: readonly FileEntry[], error: string | null) {
  const remaining = error ? entries.filter(entry => selected.includes(entry.path)) : []
  const oldIndex = oldEntries.findIndex(entry => entry.path === clickedPath)
  const nearby = remaining[0] ?? entries[Math.min(Math.max(oldIndex, 0), entries.length - 1)]
  return { remaining, nearby }
}
export function folderTrashBatch(listing: DirectoryListing): TrashBatchResult { return { listing, movedCount: 1, error: null } }
