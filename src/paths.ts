export function displayPath(path: string) {
  const devicePrefix = '\\\\?\\'
  const uncPrefix = `${devicePrefix}UNC\\`
  if (path.slice(0, uncPrefix.length).toUpperCase() === uncPrefix.toUpperCase()) return '\\\\' + path.slice(uncPrefix.length)
  if (path.startsWith(devicePrefix) && /^[A-Za-z]:\\/.test(path.slice(devicePrefix.length))) return path.slice(devicePrefix.length)
  return path
}
