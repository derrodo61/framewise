import { useEffect, useEffectEvent, useRef, useState } from 'react'
import type { PointerEvent } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ask } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { error as logError } from '@tauri-apps/plugin-log'
import { versionedMediaSrc, displayPath } from './paths'
import { adjacentImage, clampImage, fitImage, zoomImage } from './imageViewerGeometry'
import type { ImageTransform, Size } from './imageViewerGeometry'
import './image-viewer.css'

type Session = { id: number; images: { path: string; name: string }[]; index: number }
type LoadedImage = { version: string; url: string; dimensions: Size | null; error: string | null }
const unknownSize = { width: 1, height: 1 }

function ImageCanvas({ session, index, fullscreen, onFullscreen, onClose, onNavigate, onDelete, locked }: { session: Session; index: number; fullscreen: boolean; onFullscreen: () => void; onClose: () => void; onNavigate: (direction: number) => void; onDelete: (version: string) => void; locked: boolean }) {
  const stage = useRef<HTMLElement>(null)
  const drag = useRef<{ pointer: number; x: number; y: number; transform: ImageTransform } | null>(null)
  const [image, setImage] = useState<LoadedImage | null>(null)
  const [viewport, setViewport] = useState<Size>(unknownSize)
  const [manual, setManual] = useState<ImageTransform | null>(null)
  const [dragging, setDragging] = useState(false)
  const [reload, setReload] = useState(0)
  const dimensions = image?.dimensions ?? unknownSize
  const transform = manual ? clampImage(manual, dimensions, viewport) : fitImage(dimensions, viewport)
  const ready = !!image?.dimensions && !image.error
  const canPan = ready && !locked && (dimensions.width * transform.scale > viewport.width || dimensions.height * transform.scale > viewport.height)

  useEffect(() => {
    let active = true
    void invoke<{ imagePath: string; version: string }>('prepare_viewer_image', { sessionId: session.id, index })
      .then(value => { if (active) { setManual(null); setImage({ version: value.version, url: versionedMediaSrc(value.imagePath, value.version), dimensions: null, error: null }) } })
      .catch(cause => { if (active) setImage({ version: '', url: '', dimensions: null, error: String(cause) }) })
    return () => { active = false }
  }, [session.id, index, reload])
  useEffect(() => {
    const element = stage.current
    if (!element) return
    const observer = new ResizeObserver(([entry]) => setViewport({ width: entry.contentRect.width, height: entry.contentRect.height }))
    observer.observe(element)
    return () => observer.disconnect()
  }, [])

  function zoom(factor: number, anchor = { x: 0, y: 0 }) {
    if (ready && !locked) setManual(zoomImage(transform, transform.scale * factor, anchor, dimensions, viewport))
  }
  const wheel = useEffectEvent((event: WheelEvent) => {
    event.preventDefault()
    const bounds = stage.current?.getBoundingClientRect()
    if (bounds && event.deltaY !== 0) zoom(Math.exp(-Math.max(-100, Math.min(100, event.deltaY)) * 0.0025), { x: event.clientX - bounds.left - bounds.width / 2, y: event.clientY - bounds.top - bounds.height / 2 })
  })
  useEffect(() => {
    const element = stage.current
    if (!element) return
    const handler = (event: WheelEvent) => wheel(event)
    element.addEventListener('wheel', handler, { passive: false })
    return () => element.removeEventListener('wheel', handler)
  }, [])
  const keyDown = useEffectEvent((event: KeyboardEvent) => {
    if (event.altKey || event.ctrlKey || event.metaKey || event.target instanceof HTMLElement && event.target.closest('input,select,textarea,[contenteditable=true]')) return
    const key = event.key
    if (locked) return
    if (key === 'Delete' || key === 'Backspace' && /Mac/i.test(navigator.platform)) {
      event.preventDefault()
      if (!event.repeat && ready && image) onDelete(image.version)
      return
    }
    if (!['ArrowLeft', 'ArrowRight', 'Escape', 'F11', 'f', 'F', '+', '=', '-', '0', '1'].includes(key)) return
    event.preventDefault()
    if (key === 'ArrowLeft' || key === 'ArrowRight') { if (!event.repeat) onNavigate(key === 'ArrowLeft' ? -1 : 1); return }
    if (key === '+' || key === '=') { zoom(1.25); return }
    if (key === '-') { zoom(1 / 1.25); return }
    if (event.repeat) return
    if (key === '0') setManual(null)
    else if (key === '1' && ready) setManual({ scale: 1, offset: { x: 0, y: 0 } })
    else if (key === 'Escape') { if (fullscreen) onFullscreen(); else onClose() }
    else onFullscreen()
  })
  useEffect(() => {
    const handler = (event: KeyboardEvent) => keyDown(event)
    window.addEventListener('keydown', handler)
    return () => window.removeEventListener('keydown', handler)
  }, [])

  function startPan(event: PointerEvent<HTMLElement>) {
    if (!canPan || event.button !== 0) return
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    drag.current = { pointer: event.pointerId, x: event.clientX, y: event.clientY, transform }
    setDragging(true)
  }
  function pan(event: PointerEvent<HTMLElement>) {
    const start = drag.current
    if (!start || start.pointer !== event.pointerId) return
    setManual(clampImage({ scale: start.transform.scale, offset: { x: start.transform.offset.x + event.clientX - start.x, y: start.transform.offset.y + event.clientY - start.y } }, dimensions, viewport))
  }
  function stopPan() { drag.current = null; setDragging(false) }
  return <>
    <div className="image-viewer-tools" role="toolbar" aria-label="Image controls">
      <button disabled={!ready || locked} aria-pressed={manual === null} onClick={() => setManual(null)}>Fit</button>
      <button disabled={!ready || locked} aria-pressed={manual?.scale === 1} onClick={() => setManual({ scale: 1, offset: { x: 0, y: 0 } })}>100%</button>
      <button disabled={!ready || locked} aria-label="Zoom out" onClick={() => zoom(1 / 1.25)}>−</button>
      <span className="viewer-zoom" aria-live="polite">{ready ? `${Math.round(transform.scale * 100)}%` : '—'}</span>
      <button disabled={!ready || locked} aria-label="Zoom in" onClick={() => zoom(1.25)}>+</button>
      <button disabled={!ready || locked} onClick={() => { if (image) onDelete(image.version) }}>Move to Trash</button>
      <span className="viewer-image-size">{ready ? `${dimensions.width} × ${dimensions.height}` : ''}</span>
      <button className="viewer-fullscreen" disabled={locked} aria-pressed={fullscreen} onClick={onFullscreen}>{fullscreen ? 'Exit fullscreen' : 'Fullscreen'}</button>
    </div>
    <main ref={stage} className={`image-viewer-stage ${canPan ? dragging ? 'panning' : 'can-pan' : ''}`} aria-label="Image" onPointerDown={startPan} onPointerMove={pan} onPointerUp={stopPan} onPointerCancel={stopPan} onLostPointerCapture={stopPan} onDoubleClick={() => { if (ready && !locked) setManual(manual ? null : { scale: 1, offset: { x: 0, y: 0 } }) }}>
      {image?.url && !image.error && <img key={image.url} src={image.url} alt={session.images[index].name} draggable={false} style={{ visibility: ready ? 'visible' : 'hidden', width: dimensions.width, height: dimensions.height, transform: `translate(-50%, -50%) translate(${transform.offset.x}px, ${transform.offset.y}px) scale(${transform.scale})` }} onLoad={event => { const element = event.currentTarget; setImage(previous => previous ? { ...previous, dimensions: { width: element.naturalWidth, height: element.naturalHeight } } : previous) }} onError={() => setImage(previous => previous ? { ...previous, error: 'This image could not be displayed. It may have been moved, deleted, or use an unsupported encoding.' } : previous)} />}
      {!ready && <div className="viewer-image-message" role={image?.error ? 'alert' : 'status'}>{image?.error ?? 'Loading image…'}{image?.error && <button onClick={() => { setImage(null); setReload(value => value + 1) }}>Retry</button>}</div>}
    </main>
  </>
}

export default function ImageViewer() {
  const [selection, setSelection] = useState<{ session: Session; index: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [fullscreen, setFullscreen] = useState(false)
  const fullscreenBusy = useRef(false)
  const deletionPending = useRef(false)
  const [deleting, setDeleting] = useState(false)
  function report(cause: unknown) { const message = String(cause); setError(message); void logError(`Image viewer: ${message}`).catch(() => {}) }
  useEffect(() => {
    let active = true
    let unlisten: (() => void) | undefined
    const apply = (session: Session) => {
      if (active) { setSelection(previous => previous && previous.session.id >= session.id ? previous : { session, index: session.index }); setError(null) }
    }
    void listen<Session>('framewise:image-viewer-session', event => apply(event.payload)).then(async stop => {
      if (!active) { stop(); return }
      unlisten = stop
      apply(await invoke<Session>('image_viewer_session'))
    }).catch(cause => { if (active) report(cause) })
    return () => { active = false; unlisten?.() }
  }, [])
  useEffect(() => {
    let active = true
    let stop: (() => void) | undefined
    const update = () => { void getCurrentWindow().isFullscreen().then(value => { if (active) setFullscreen(value) }).catch(cause => { if (active) report(cause) }) }
    update()
    void getCurrentWindow().onResized(update).then(unlisten => { if (active) stop = unlisten; else unlisten() }).catch(cause => { if (active) report(cause) })
    return () => { active = false; stop?.() }
  }, [])
  const image = selection?.session.images[selection.index]
  useEffect(() => {
    if (image) void getCurrentWindow().setTitle(`${image.name} — Framewise`).catch(cause => { void logError(`Image viewer title: ${String(cause)}`).catch(() => {}) })
  }, [image])
  async function toggleFullscreen() {
    if (fullscreenBusy.current) return
    fullscreenBusy.current = true
    try { const value = !await getCurrentWindow().isFullscreen(); await getCurrentWindow().setFullscreen(value); setFullscreen(value); setError(null) }
    catch (cause) { report(cause) }
    finally { fullscreenBusy.current = false }
  }
  function close() { if (deletionPending.current) return; void getCurrentWindow().close().catch(report) }
  function navigate(direction: number) { if (deletionPending.current) return; setSelection(previous => previous ? { ...previous, index: adjacentImage(previous.index, previous.session.images.length, direction) } : previous) }
  async function deleteImage(version: string) {
    if (!selection || !image || deletionPending.current) return
    deletionPending.current = true; setDeleting(true); setError(null)
    const request = { sessionId: selection.session.id, index: selection.index, version }
    try {
      const agreed = await ask(`Move “${image.name}” to Trash/Recycle Bin? You can restore it from your system's Trash or Recycle Bin.`, { title: 'Move image to Trash', kind: 'warning', okLabel: 'Move to Trash', cancelLabel: 'Cancel' })
      if (!agreed) return
      const result = await invoke<{ session: Session; warning: string | null }>('trash_viewer_image', request)
      setSelection(previous => previous && previous.session.id > result.session.id ? previous : { session: result.session, index: result.session.index })
      if (result.warning) report(result.warning)
      if (!result.session.images.length && !result.warning) await getCurrentWindow().close()
    } catch (cause) { report(cause) }
    finally { deletionPending.current = false; setDeleting(false) }
  }
  return <div className="image-viewer">
    <header className="image-viewer-header">
      <div><span>FRAMEWISE · IMAGE VIEWER</span><h1 title={image ? displayPath(image.path) : undefined}>{image?.name ?? 'Image viewer'}</h1></div>
      <nav aria-label="Browse images"><button aria-label="Previous image" disabled={deleting || !selection || selection.index === 0} onClick={() => navigate(-1)}>←</button><span>{selection ? `${selection.index + 1} / ${selection.session.images.length}` : '—'}</span><button aria-label="Next image" disabled={deleting || !selection || !selection.session.images.length || selection.index === selection.session.images.length - 1} onClick={() => navigate(1)}>→</button></nav>
      <button disabled={deleting} onClick={close}>Close</button>
    </header>
    {error && <div className="viewer-window-error" role="alert">{error}</div>}
    {selection && selection.session.images.length > 0 ? <ImageCanvas key={`${selection.session.id}:${selection.index}`} session={selection.session} index={selection.index} fullscreen={fullscreen} onFullscreen={() => void toggleFullscreen()} onClose={close} onNavigate={navigate} onDelete={version => void deleteImage(version)} locked={deleting} /> : <main className="image-viewer-stage"><p className="viewer-image-message">{selection && !selection.session.images.length ? 'No images remain in this viewer. You can close this window.' : error ? 'Close this window and open the image viewer again from the inspector.' : 'Opening image…'}</p></main>}
    <footer>← → Browse images · Scroll to zoom · Drag to pan · 0 Fit · 1 Actual size · Delete Move to Trash · F Fullscreen · Esc {fullscreen ? 'exit fullscreen' : 'close viewer'}</footer>
  </div>
}
