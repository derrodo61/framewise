export type DateFilterState = { field: 'modified' | 'created'; preset: 'all' | 'today' | 'yesterday' | '7' | '30' | 'custom'; from: string; to: string }
export type DateBounds = { field: 'modified' | 'created'; from: number | null; to: number | null; error: string | null }
export const emptyDateFilter: DateFilterState = { field: 'modified', preset: 'all', from: '', to: '' }
export function restoreDateFilter(raw: string | null): { root: string | null; value: DateFilterState } {
  try {
    const saved = JSON.parse(raw ?? 'null')
    const value = saved?.value
    if (typeof saved?.root === 'string' && value && ['modified', 'created'].includes(value.field)
      && ['all', 'today', 'yesterday', '7', '30', 'custom'].includes(value.preset) && typeof value.from === 'string' && typeof value.to === 'string') return { root: saved.root, value }
  } catch { /* Invalid preferences do not become active filters. */ }
  return { root: null, value: emptyDateFilter }
}
function localDate(value: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return null
  const [year, month, day] = value.split('-').map(Number)
  const date = new Date(year, month - 1, day)
  return date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day ? date : null
}
export function dateBounds(filter: DateFilterState, now = new Date()): DateBounds {
  const bounds: DateBounds = { field: filter.field, from: null, to: null, error: null }
  if (filter.preset === 'all') return bounds
  if (filter.preset !== 'custom') {
    const offset = filter.preset === 'yesterday' ? -1 : filter.preset === '7' ? -6 : filter.preset === '30' ? -29 : 0
    bounds.from = new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset).getTime()
    bounds.to = new Date(now.getFullYear(), now.getMonth(), now.getDate() + (filter.preset === 'yesterday' ? 0 : 1)).getTime()
    return bounds
  }
  const from = filter.from ? localDate(filter.from) : null
  const to = filter.to ? localDate(filter.to) : null
  if ((filter.from && !from) || (filter.to && !to)) return { ...bounds, from: 1, to: 0, error: 'Enter valid dates.' }
  bounds.from = from?.getTime() ?? null
  bounds.to = to ? new Date(to.getFullYear(), to.getMonth(), to.getDate() + 1).getTime() : null
  if (bounds.from !== null && bounds.to !== null && bounds.from >= bounds.to) bounds.error = 'From must be on or before To.'
  return bounds
}
export function filterMediaDates<T extends { isDirectory: boolean; modifiedAt: number | null; createdAt?: number | null }>(entries: readonly T[], bounds: DateBounds): T[] {
  const active = bounds.from !== null || bounds.to !== null
  return entries.filter(entry => {
    if (entry.isDirectory) return true
    if (bounds.error) return false
    if (!active) return true
    const value = bounds.field === 'created' ? entry.createdAt : entry.modifiedAt
    return value !== null && value !== undefined && (bounds.from === null || value >= bounds.from) && (bounds.to === null || value < bounds.to)
  })
}
