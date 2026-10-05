import { useEffect, useRef, useState } from 'react'
import type { ReactNode } from 'react'
import { convertFileSrc, invoke } from '@tauri-apps/api/core'
import { generateThumbnail } from './thumbnailQueue'

export default function MediaThumbnail({ path, revision, placeholder }: { path: string; revision: object; placeholder: ReactNode }) {
  const element = useRef<HTMLSpanElement>(null)
  const [visible, setVisible] = useState(false)
  const [result, setResult] = useState<{ path: string; revision: object; image: string | null; unavailable: boolean } | null>(null)
  const current = result?.path === path && result.revision === revision ? result : null
  const image = current?.image ?? null
  const unavailable = current?.unavailable ?? false

  useEffect(() => {
    const observer = new IntersectionObserver(entries => setVisible(entries[0].isIntersecting), { rootMargin: '180px' })
    if (element.current) observer.observe(element.current)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (!visible || image || unavailable) return
    let active = true
    void invoke<{ thumbnailPath: string | null }>('prepare_preview', { path })
      .then(async preview => {
        if (!active) return
        const thumbnail = preview.thumbnailPath ?? await generateThumbnail(path, () => active)
        if (active) {
          setResult({ path, revision, image: thumbnail ? convertFileSrc(thumbnail) : null, unavailable: !thumbnail })
        }
      })
      .catch(() => { if (active) setResult({ path, revision, image: null, unavailable: true }) })
    return () => { active = false }
  }, [path, revision, visible, image, unavailable])

  return <span ref={element} className="media-thumbnail">
    {image ? <img src={image} alt="" onError={() => setResult({ path, revision, image: null, unavailable: true })} /> : <>{placeholder}<span className="thumbnail-status">{unavailable ? 'Preview unavailable' : visible ? 'Preparing preview…' : ''}</span></>}
  </span>
}
