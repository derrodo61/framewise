import { useEffect, useId, useMemo, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { error as logError } from '@tauri-apps/plugin-log'
import './video-tags.css'

export type TagVideo = { videoId: number; path: string }
export type NamedTagVideo = TagVideo & { name: string }
type SelectionTag = { id: number; name: string; assignedCount: number }
type TagSelection = { videoCount: number; tags: SelectionTag[] }
function report(cause: unknown) {
  const message = cause instanceof Error ? cause.message : String(cause)
  void logError(`Assigning video tags: ${message}`).catch(() => {})
  return message
}

export function VideoTags({ videos, refreshToken = 0, onChanged, onBusyChange }: { videos: TagVideo[]; refreshToken?: number; onChanged: () => void; onBusyChange?: (busy: boolean) => void }) {
  const scope = JSON.stringify(videos)
  const targets = useMemo(() => JSON.parse(scope) as TagVideo[], [scope])
  const id = useId()
  const alive = useRef(true)
  const request = useRef(0)
  const [snapshot, setSnapshot] = useState<TagSelection | null>(null)
  const [revision, setRevision] = useState(0)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [chosenTag, setChosenTag] = useState('')
  const [newName, setNewName] = useState('')
  useEffect(() => { alive.current = true; return () => { alive.current = false } }, [])
  useEffect(() => {
    let active = true
    const current = ++request.current
    void invoke<TagSelection>('selection_tags', { videos: targets })
      .then(result => { if (active && current === request.current) setSnapshot(result) })
      .catch(cause => { if (active && current === request.current) { setSnapshot(null); setError(report(cause)) } })
      .finally(() => { if (active && current === request.current) setLoading(false) })
    return () => { active = false }
  }, [targets, revision, refreshToken])

  function refresh() { setLoading(true); setError(null); setRevision(value => value + 1) }
  async function mutate(command: string, args: Record<string, unknown>, message: string) {
    if (busy || loading) return
    request.current++
    setBusy(true); onBusyChange?.(true); setError(null); setNotice(null)
    try {
      const result = await invoke<TagSelection>(command, { videos: targets, ...args })
      if (alive.current) {
        request.current++
        setSnapshot(result); setLoading(false); setChosenTag(''); setNewName(''); setNotice(message); onChanged()
      }
    } catch (cause) {
      if (alive.current) { setError(report(cause)); setLoading(true); setRevision(value => value + 1) }
    } finally { if (alive.current) { setBusy(false); onBusyChange?.(false) } }
  }
  const count = snapshot?.videoCount ?? targets.length
  const batch = count > 1
  const assigned = snapshot?.tags.filter(tag => tag.assignedCount > 0) ?? []
  const available = snapshot?.tags.filter(tag => tag.assignedCount < count) ?? []
  const disabled = loading || busy || !snapshot
  const addMessage = batch ? `Tag added to all ${count} selected videos.` : 'Tag added.'
  const removeMessage = batch ? `Tag removed from all ${count} selected videos.` : 'Tag removed.'

  return <div className="video-tag-editor" aria-busy={loading || busy}>
    <p className="video-tag-help">{batch ? `Changes apply to all ${count} selected videos.` : 'These tags apply to this video only.'}</p>
    {loading && <p className="video-tag-help" role="status">Loading tags…</p>}
    {busy && <p className="video-tag-help" role="status">Saving tags…</p>}
    {error && <p className="video-tag-error" role="alert">{error}</p>}
    {notice && <p className="video-tag-notice" role="status">{notice}</p>}
    <ul className="assigned-video-tags" aria-label="Assigned tags">
      {assigned.map(tag => <li key={tag.id}><div className="video-tag-name"><strong>{tag.name}</strong>{batch && <span>{tag.assignedCount === count ? `All (${count})` : `Some (${tag.assignedCount} of ${count})`}</span>}</div><div className="video-tag-actions">
        {batch && tag.assignedCount < count && <button disabled={disabled} onClick={() => void mutate('set_video_tag', { tagId: tag.id, assigned: true }, addMessage)} aria-label={`Add ${tag.name} to all selected videos`}>Add to all</button>}
        <button disabled={disabled} onClick={() => void mutate('set_video_tag', { tagId: tag.id, assigned: false }, removeMessage)} aria-label={batch ? `Remove ${tag.name} from all selected videos` : `Remove tag ${tag.name}`}>{batch ? 'Remove from all' : 'Remove'}</button>
      </div></li>)}
    </ul>
    {!loading && snapshot && assigned.length === 0 && <p className="video-tag-help">{batch ? 'None of these videos have tags yet.' : 'No tags assigned yet.'}</p>}
    <form onSubmit={event => { event.preventDefault(); if (chosenTag) void mutate('set_video_tag', { tagId: Number(chosenTag), assigned: true }, addMessage) }}>
      <label htmlFor={`${id}-existing`}>Add an existing tag</label>
      <div className="video-tag-input"><select id={`${id}-existing`} value={chosenTag} onChange={event => setChosenTag(event.target.value)} disabled={disabled || available.length === 0}><option value="">{available.length ? 'Choose a tag…' : 'No other tags available'}</option>{available.map(tag => <option key={tag.id} value={tag.id}>{tag.name}{batch && tag.assignedCount > 0 ? ` (${tag.assignedCount} of ${count})` : ''}</option>)}</select><button type="submit" disabled={disabled || !chosenTag}>{batch ? 'Add to all' : 'Add'}</button></div>
    </form>
    <form onSubmit={event => { event.preventDefault(); if (newName.trim()) void mutate('create_and_assign_tag', { name: newName }, addMessage) }}>
      <label htmlFor={`${id}-new`}>Create and add a tag</label>
      <div className="video-tag-input"><input id={`${id}-new`} placeholder="Tag name" value={newName} onChange={event => setNewName(event.target.value)} disabled={disabled} /><button type="submit" disabled={disabled || !newName.trim()}>{batch ? 'Add to all' : 'Add'}</button></div>
      <p className="video-tag-help">An existing name reuses that tag.</p>
    </form>
    <button className="video-tag-refresh" onClick={refresh} disabled={busy || loading}>Refresh tags</button>
    <p className="video-tag-help">Tags are saved locally without changing the video file.</p>
  </div>
}

export function TagBatchDialog({ videos, onChanged, onClose }: { videos: NamedTagVideo[]; onChanged: () => void; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    const element = dialog.current
    if (element && !element.open) element.showModal()
    return () => { element?.close() }
  }, [])
  return <dialog ref={dialog} className="video-tag-dialog" aria-labelledby="batch-tags-title" onCancel={event => { event.preventDefault(); if (!busy) onClose() }}>
    <header><h2 id="batch-tags-title">Edit tags · {videos.length} {videos.length === 1 ? 'video' : 'videos'}</h2><button onClick={onClose} disabled={busy} aria-label="Close tag editor">Close</button></header>
    <details className="tag-selected-videos"><summary>Selected videos</summary><ul>{videos.map(video => <li key={video.videoId} title={video.path}>{video.name}</li>)}</ul></details>
    <VideoTags videos={videos.map(({ videoId, path }) => ({ videoId, path }))} onChanged={onChanged} onBusyChange={setBusy} />
    <footer><span>Changes are saved as you make them.</span><button onClick={onClose} disabled={busy}>Done</button></footer>
  </dialog>
}
