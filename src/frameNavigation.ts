const FRAME_EPSILON = 0.0001

function lowerBound(times: number[], target: number) {
  let low = 0
  let high = times.length
  while (low < high) {
    const middle = (low + high) >>> 1
    if (times[middle] < target) low = middle + 1
    else high = middle
  }
  return low
}

function upperBound(times: number[], target: number) {
  let low = 0
  let high = times.length
  while (low < high) {
    const middle = (low + high) >>> 1
    if (times[middle] <= target) low = middle + 1
    else high = middle
  }
  return low
}

export function frameIndexAt(times: number[], seconds: number) {
  return Math.max(0, upperBound(times, seconds + FRAME_EPSILON) - 1)
}

export function adjacentFrameTime(times: number[], seconds: number, direction: -1 | 1) {
  if (times.length === 0) return null
  const index = direction > 0
    ? upperBound(times, seconds + FRAME_EPSILON)
    : lowerBound(times, seconds - FRAME_EPSILON) - 1
  return times[Math.max(0, Math.min(index, times.length - 1))]
}
