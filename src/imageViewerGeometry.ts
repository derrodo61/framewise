export type Size = { width: number; height: number }
export type Point = { x: number; y: number }
export type ImageTransform = { scale: number; offset: Point }
export function fitImage(image: Size, viewport: Size): ImageTransform {
  const scale = Math.min(1, Math.max(1, viewport.width - 32) / image.width, Math.max(1, viewport.height - 32) / image.height)
  return { scale, offset: { x: 0, y: 0 } }
}
export function clampImage(transform: ImageTransform, image: Size, viewport: Size): ImageTransform {
  const x = Math.max(0, (image.width * transform.scale - viewport.width) / 2)
  const y = Math.max(0, (image.height * transform.scale - viewport.height) / 2)
  return { scale: transform.scale, offset: { x: x === 0 ? 0 : Math.max(-x, Math.min(x, transform.offset.x)), y: y === 0 ? 0 : Math.max(-y, Math.min(y, transform.offset.y)) } }
}
// Anchor coordinates are relative to the viewport's centre.
export function zoomImage(current: ImageTransform, requestedScale: number, anchor: Point, image: Size, viewport: Size): ImageTransform {
  const minimum = Math.min(0.05, fitImage(image, viewport).scale)
  const scale = Math.max(minimum, Math.min(16, requestedScale))
  const ratio = scale / current.scale
  return clampImage({ scale, offset: { x: anchor.x - (anchor.x - current.offset.x) * ratio, y: anchor.y - (anchor.y - current.offset.y) * ratio } }, image, viewport)
}
export function adjacentImage(index: number, count: number, direction: number): number {
  return Math.max(0, Math.min(Math.max(0, count - 1), index + direction))
}
