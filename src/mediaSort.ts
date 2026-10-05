export type MediaSort = 'name' | 'modified'
export type SortDirection = 'asc' | 'desc'
type SortableEntry = { name: string; path: string; isDirectory: boolean; modifiedAt: number | null }
const names = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

export function sortMedia<T extends SortableEntry>(entries: readonly T[], criterion: MediaSort, direction: SortDirection): T[] {
  const sign = direction === 'asc' ? 1 : -1
  return [...entries].sort((a, b) => {
    if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1
    if (criterion === 'modified') {
      // Missing dates stay at the end in either direction.
      if (a.modifiedAt === null && b.modifiedAt !== null) return 1
      if (b.modifiedAt === null && a.modifiedAt !== null) return -1
      const dateOrder = (a.modifiedAt ?? 0) - (b.modifiedAt ?? 0)
      if (dateOrder) return dateOrder * sign
    }
    return (names.compare(a.name, b.name) || a.path.localeCompare(b.path)) * sign
  })
}
