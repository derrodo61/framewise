export function cutPreviewAction(time: number, start: number, end: number, duration: number, armed: boolean) {
  if (!armed || time < start) return null
  return end >= duration - 0.01 ? 'stop' : 'skip'
}
