type Entry = { path: string; isDirectory: boolean; videoId?: number | null }
export function filterMedia<T extends Entry>(entries: readonly T[], active: boolean, matchingIds: readonly number[]): T[] {
  if (!active) return [...entries]
  const ids = new Set(matchingIds)
  return entries.filter(entry => entry.isDirectory || (entry.videoId != null && ids.has(entry.videoId)))
}
export function visibleSelection(paths: readonly string[], entries: readonly Entry[]): string[] {
  const visible = new Set(entries.map(entry => entry.path))
  return paths.filter(path => visible.has(path))
}
