import type { FileEntry } from './mediaModel'
type Modifiers = { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }
export function nextSelection(entries: readonly FileEntry[], selected: readonly string[], anchorPath: string | null, entry: FileEntry, index: number, modifiers: Modifiers) {
  const additive = modifiers.ctrlKey || modifiers.metaKey
  if (entry.isDirectory && !additive && !modifiers.shiftKey) return { openFolder: entry.path, paths: [...selected], anchor: anchorPath, inspect: undefined }
  const compatible = selected.filter(path => entries.some(item => item.path === path && item.isDirectory === entry.isDirectory))
  if (modifiers.shiftKey) {
    const anchor = entries.findIndex(item => item.path === anchorPath && item.isDirectory === entry.isDirectory)
    const from = anchor < 0 ? index : anchor
    const range = entries.slice(Math.min(from, index), Math.max(from, index) + 1).filter(item => item.isDirectory === entry.isDirectory).map(item => item.path)
    return { openFolder: null, paths: additive ? [...new Set([...compatible, ...range])] : range, anchor: anchor < 0 ? entry.path : anchorPath, inspect: entry }
  }
  const paths = additive ? compatible.includes(entry.path) ? compatible.filter(path => path !== entry.path) : [...compatible, entry.path] : [entry.path]
  return { openFolder: null, paths, anchor: entry.path, inspect: paths.includes(entry.path) ? entry : entries.find(item => item.path === paths.at(-1)) }
}
