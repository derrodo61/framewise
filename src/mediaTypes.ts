export function isImage(path: string): boolean { return /\.(jpe?g|png|webp)$/i.test(path) }
export function filterMediaTypes<T extends { path: string; isDirectory: boolean }>(entries: readonly T[], videos: boolean, images: boolean): T[] {
  return entries.filter(entry => entry.isDirectory || (isImage(entry.path) ? images : videos))
}
