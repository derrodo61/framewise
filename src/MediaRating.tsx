import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { error as logError } from '@tauri-apps/plugin-log'
import type { NamedTagVideo } from './VideoTags'
import './ratings.css'

export type RatingUpdate = { videoId: number; rating: number | null }
export function MediaRating({ files, rating, mixed = false, disabled = false, onChanged, onBusyChange }: { files: NamedTagVideo[]; rating: number | null; mixed?: boolean; disabled?: boolean; onChanged: (updates: RatingUpdate[]) => void; onBusyChange?: (busy: boolean) => void }) {
  const alive = useRef(true)
  const pending = useRef(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  async function save(value: number | null) {
    if (pending.current || disabled) return
    pending.current = true; setBusy(true); setError(null); onBusyChange?.(true)
    try {
      const updates = await invoke<RatingUpdate[]>('set_media_rating', { videos: files.map(({ videoId, path }) => ({ videoId, path })), rating: value })
      if (alive.current) onChanged(updates)
    } catch (cause) {
      const message = String(cause); void logError(`Saving rating: ${message}`).catch(() => {})
      if (alive.current) setError(message)
    } finally { pending.current = false; if (alive.current) { setBusy(false); onBusyChange?.(false) } }
  }
  return <div className="media-rating" aria-busy={busy}>
    <div className="rating-controls" role="group" aria-label={files.length > 1 ? 'Rate selected files' : 'Rate this file'}>
      {[1, 2, 3, 4, 5].map(stars => <button key={stars} className={`rating-star ${!mixed && rating != null && stars <= rating ? 'filled' : ''}`} aria-label={`Set ${stars} ${stars === 1 ? 'star' : 'stars'}${files.length > 1 ? ' for all selected files' : ''}`} aria-pressed={!mixed && stars === rating} title={`${stars} ${stars === 1 ? 'star' : 'stars'}`} disabled={busy || disabled} onClick={() => void save(stars)}>★</button>)}
      <span className="rating-status">{busy ? 'Saving…' : mixed ? 'Mixed ratings' : rating == null ? 'Unrated' : `${rating}/5`}</span>
      <button className="rating-clear" disabled={busy || disabled || !mixed && rating == null} onClick={() => void save(null)}>Clear</button>
    </div>
    {error && <p role="alert" className="video-tag-error">{error}</p>}
  </div>
}
export function RatingDialog({ files, onChanged, onClose }: { files: (NamedTagVideo & { rating?: number | null })[]; onChanged: (updates: RatingUpdate[]) => void; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const [busy, setBusy] = useState(false)
  const [values, setValues] = useState(files)
  useEffect(() => { const element = dialog.current; element?.showModal(); return () => element?.close() }, [])
  const rating = values[0]?.rating ?? null
  const mixed = values.some(file => (file.rating ?? null) !== rating)
  return <dialog ref={dialog} className="video-tag-dialog" aria-labelledby="rating-dialog-title" onCancel={event => { event.preventDefault(); if (!busy) onClose() }}>
    <header><h2 id="rating-dialog-title">Rate {files.length} {files.length === 1 ? 'file' : 'files'}</h2><button disabled={busy} onClick={onClose}>Close</button></header>
    <p className="video-tag-help">Changes apply to all selected files and are saved immediately.</p>
    <MediaRating files={files} rating={rating} mixed={mixed} onBusyChange={setBusy} onChanged={updates => { setValues(previous => previous.map(file => ({ ...file, rating: updates.find(update => update.videoId === file.videoId)?.rating ?? null }))); onChanged(updates) }} />
    <details className="tag-selected-videos"><summary>Selected files</summary><ul>{files.map(file => <li key={file.videoId} title={file.path}>{file.name}</li>)}</ul></details>
    <footer><span>Ratings are saved locally.</span><button disabled={busy} onClick={onClose}>Done</button></footer>
  </dialog>
}
