export type RatingFilterState = { mode: 'all' | 'unrated' | 'exactly' | 'atLeast' | 'moreThan'; value: number }
export const allRatings: RatingFilterState = { mode: 'all', value: 3 }
export function restoreRatingFilter(raw: string | null): RatingFilterState {
  try {
    const value = JSON.parse(raw ?? '') as RatingFilterState
    if (value && ['all', 'unrated', 'exactly', 'atLeast', 'moreThan'].includes(value.mode) && Number.isInteger(value.value) && value.value >= 1 && value.value <= 5) return value
  } catch { /* Invalid saved filters reset to all ratings. */ }
  return allRatings
}
export function matchesRating(rating: number | null | undefined, filter: RatingFilterState = allRatings) {
  if (filter.mode === 'all') return true
  if (filter.mode === 'unrated') return rating == null
  if (rating == null) return false
  return filter.mode === 'exactly' ? rating === filter.value : filter.mode === 'atLeast' ? rating >= filter.value : rating > filter.value
}
export function filterRatings<T extends { isDirectory: boolean; rating?: number | null }>(entries: T[], filter: RatingFilterState = allRatings): T[] {
  return entries.filter(entry => entry.isDirectory || matchesRating(entry.rating, filter))
}
export function ratingFilterLabel(filter: RatingFilterState) {
  return filter.mode === 'all' ? 'All ratings' : filter.mode === 'unrated' ? 'Unrated only' : `${filter.mode === 'exactly' ? 'Exactly' : filter.mode === 'atLeast' ? 'At least' : 'More than'} ${filter.value} star${filter.value === 1 ? '' : 's'}`
}
