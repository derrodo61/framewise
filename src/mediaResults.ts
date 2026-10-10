import { filterRatings } from './mediaRatings'
import type { RatingFilterState } from './mediaRatings'
import type { FileEntry } from './mediaModel'
import type { DateBounds } from './mediaDates'
import { filterMediaDates } from './mediaDates'
import { filterMediaTypes } from './mediaTypes'
import { filterMedia } from './mediaFilter'
import { sortMedia } from './mediaSort'
import type { MediaSort, SortDirection } from './mediaSort'

export type ResultFilters = { rating?: RatingFilterState; showVideos: boolean; showImages: boolean; dates: DateBounds; tagActive: boolean; matchingIds: readonly number[]; sort: MediaSort; direction: SortDirection }
export function folderResults(entries: readonly FileEntry[], filters: ResultFilters): FileEntry[] {
  return filterRatings(filterMedia(filterMediaDates(filterMediaTypes(sortMedia(entries, filters.sort, filters.direction), filters.showVideos, filters.showImages), filters.dates), filters.tagActive, filters.matchingIds), filters.rating)
}
