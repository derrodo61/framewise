type Entry = { path: string; isDirectory: boolean; videoId?: number | null }
export function parentPath(path: string): string {
  const index = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))
  const parent = path.slice(0, index)
  return index === 0 || parent.endsWith(':') ? path.slice(0, index + 1) : parent
}
export function sameParent(paths: readonly string[]): boolean {
  return new Set(paths.map(parentPath)).size <= 1
}
export function filterMedia<T extends Entry>(entries: readonly T[], active: boolean, matchingIds: readonly number[]): T[] {
  if (!active) return [...entries]
  const ids = new Set(matchingIds)
  return entries.filter(entry => entry.isDirectory || (entry.videoId != null && ids.has(entry.videoId)))
}
export function visibleSelection(paths: readonly string[], entries: readonly Entry[]): string[] {
  const visible = new Set(entries.map(entry => entry.path))
  return paths.filter(path => visible.has(path))
}
