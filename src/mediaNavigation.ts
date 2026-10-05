export function nextMediaIndex(index: number, count: number, columns: number, key: string): number {
  const step = key === 'ArrowDown' ? columns : key === 'ArrowUp' ? -columns : key === 'ArrowRight' ? 1 : -1
  if (key === 'ArrowUp' && index < columns) return index
  if (key === 'ArrowDown' && index + columns >= count) {
    return Math.floor(index / columns) < Math.floor((count - 1) / columns) ? count - 1 : index
  }
  return Math.max(0, Math.min(count - 1, index + step))
}
