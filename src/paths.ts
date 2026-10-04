import { convertFileSrc } from '@tauri-apps/api/core'

export function versionedMediaSrc(path: string, version: string) {
  // The same file path can hold a newly rendered video after Save.
  const url = new URL(convertFileSrc(path))
  url.searchParams.set('version', version)
  return url.toString()
}

export function displayPath(path: string) {
  const devicePrefix = '\\\\?\\'
  const uncPrefix = `${devicePrefix}UNC\\`
  if (path.slice(0, uncPrefix.length).toUpperCase() === uncPrefix.toUpperCase()) return '\\\\' + path.slice(uncPrefix.length)
  if (path.startsWith(devicePrefix) && /^[A-Za-z]:\\/.test(path.slice(devicePrefix.length))) return path.slice(devicePrefix.length)
  return path
}
