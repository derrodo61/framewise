import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { versionedMediaSrc } from './paths'
import './image-preview.css'

export default function ImagePreview({ path, revision, thumbnail = false }: { path: string; revision: object; thumbnail?: boolean }) {
  const element = useRef<HTMLSpanElement>(null)
  const [visible, setVisible] = useState(!thumbnail)
  const [result, setResult] = useState<{ path: string; revision: object; url: string | null; error: string | null } | null>(null)
  const [dimensions, setDimensions] = useState<{ url: string; width: number; height: number } | null>(null)
  const current = result?.path === path && result.revision === revision ? result : null
  useEffect(() => {
    if (!thumbnail) return
    const observer = new IntersectionObserver(entries => setVisible(entries[0].isIntersecting), { rootMargin: '180px' })
    if (element.current) observer.observe(element.current)
    return () => observer.disconnect()
  }, [thumbnail])
  useEffect(() => {
    if (!visible) return
    let active = true
    void invoke<{ imagePath: string; version: string }>('prepare_image_preview', { path })
      .then(image => { if (active) setResult({ path, revision, url: versionedMediaSrc(image.imagePath, image.version), error: null }) })
      .catch(cause => { if (active) setResult({ path, revision, url: null, error: String(cause) }) })
    return () => { active = false }
  }, [path, revision, visible])
  return <span ref={element} className={thumbnail ? 'media-thumbnail image-thumbnail' : 'image-preview'}>
    {current?.url && !current.error && (!thumbnail || visible) ? <img src={current.url} alt={thumbnail ? '' : 'Selected image'} loading={thumbnail ? 'lazy' : 'eager'} onLoad={event => setDimensions({ url: current.url!, width: event.currentTarget.naturalWidth, height: event.currentTarget.naturalHeight })} onError={() => setResult({ ...current, error: 'This image could not be displayed. It may be damaged or use an unsupported encoding.' })} />
      : <span className="image-preview-status" role={current?.error ? 'alert' : 'status'}>{current?.error ?? (visible ? 'Loading image…' : 'Image')}</span>}
    {!thumbnail && current?.url && dimensions?.url === current.url && !current.error && <span className="image-dimensions">{dimensions.width} × {dimensions.height} pixels</span>}
  </span>
}
