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
  const [chosenTags, setChosenTags] = useState<number[]>([])
  const [tagQuery, setTagQuery] = useState('')
  const [showAll, setShowAll] = useState(false)
  const [pickerOpen, setPickerOpen] = useState(false)
  const picker = useRef<HTMLDivElement>(null)
  const addButton = useRef<HTMLButtonElement>(null)
  const search = useRef<HTMLInputElement>(null)
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

  function closePicker() { picker.current?.hidePopover(); addButton.current?.focus() }
  function openPicker() {
    const element = picker.current
    const button = addButton.current
    if (!element || !button) return
    if (element.matches(':popover-open')) { closePicker(); return }
    const bounds = button.getBoundingClientRect()
    const width = Math.min(360, window.innerWidth - 24)
    element.style.width = `${width}px`
    element.style.maxHeight = `${window.innerHeight - 24}px`
    element.style.left = `${Math.max(12, Math.min(bounds.left, window.innerWidth - width - 12))}px`
    element.style.top = `${bounds.bottom + 6}px`
    element.showPopover()
    const height = element.getBoundingClientRect().height
    if (bounds.bottom + 6 + height > window.innerHeight - 12) {
      element.style.top = `${Math.max(12, bounds.top - height - 6)}px`
    }
    search.current?.focus()
  }
  useEffect(() => {
    const dismiss = () => picker.current?.hidePopover()
    const dismissOnScroll = (event: Event) => {
      if (event.target instanceof Node && !picker.current?.contains(event.target)) dismiss()
    }
    window.addEventListener('scroll', dismissOnScroll, true)
    window.addEventListener('resize', dismiss)
    return () => { window.removeEventListener('resize', dismiss); window.removeEventListener('scroll', dismissOnScroll, true) }
  }, [])
  function refresh() { setLoading(true); setError(null); setRevision(value => value + 1) }
  async function mutate(command: string, args: Record<string, unknown>, message: string) {
    if (busy || loading) return
    request.current++
    setBusy(true); onBusyChange?.(true); setError(null); setNotice(null)
    try {
      const result = await invoke<TagSelection>(command, { videos: targets, ...args })
      if (alive.current) {
        request.current++
        setSnapshot(result); setLoading(false); setTagQuery(''); setNotice(message); onChanged()
        if (command !== 'create_and_assign_tag') setChosenTags([])
        if (command === 'add_video_tags') closePicker()
      }
    } catch (cause) {
      if (alive.current) { setError(report(cause)); setLoading(true); setRevision(value => value + 1) }
    } finally { if (alive.current) { setBusy(false); onBusyChange?.(false) } }
  }
  const count = snapshot?.videoCount ?? targets.length
  const batch = count > 1
  const assigned = snapshot?.tags.filter(tag => tag.assignedCount > 0) ?? []
  const available = snapshot?.tags.filter(tag => tag.assignedCount < count) ?? []
  const selectedTags = chosenTags.filter(tagId => available.some(tag => tag.id === tagId))
  const matchingTags = available.filter(tag => tag.name.toLocaleLowerCase().includes(tagQuery.trim().toLocaleLowerCase()))
  const queryName = tagQuery.trim().replace(/\s+/g, ' ')
  const exactTag = snapshot?.tags.find(tag => tag.name.normalize('NFKC').toLocaleLowerCase() === queryName.normalize('NFKC').toLocaleLowerCase())
  const disabled = loading || busy || !snapshot
  const visibleAssigned = showAll ? assigned : assigned.slice(0, 6)
  const addMessage = batch ? `Tag added to all ${count} selected files.` : 'Tag added.'
  const removeMessage = batch ? `Tag removed from all ${count} selected files.` : 'Tag removed.'

  return <div className="video-tag-editor" aria-busy={loading || busy}>
    {batch && <p className="video-tag-help">Changes apply to all {count} selected files.</p>}
    {loading && <p className="video-tag-help" role="status">Loading tags…</p>}
    {busy && <p className="video-tag-help" role="status">Saving tags…</p>}
    {error && <p className="video-tag-error" role="alert">{error} <button disabled={busy || loading} onClick={refresh}>Retry</button></p>}
    {notice && <p className="video-tag-notice" role="status">{notice}</p>}
    <div className="assigned-tag-field">
      <ul className="assigned-video-tags" aria-label="Assigned tags">
        {visibleAssigned.map(tag => <li key={tag.id}>
          <span className="video-tag-name" title={tag.name}>{tag.name}{batch && <span>{tag.assignedCount === count ? `All (${count})` : `${tag.assignedCount} of ${count}`}</span>}</span>
          {batch && tag.assignedCount < count && <button disabled={disabled} title={`Add ${tag.name} to all selected files`} onClick={() => void mutate('set_video_tag', { tagId: tag.id, assigned: true }, addMessage)} aria-label={`Add ${tag.name} to all selected files`}>+ All</button>}
          <button className="tag-remove" disabled={disabled} title={batch ? `Remove ${tag.name} from all selected files` : `Remove ${tag.name}`} onClick={() => void mutate('set_video_tag', { tagId: tag.id, assigned: false }, removeMessage)} aria-label={batch ? `Remove ${tag.name} from all selected files` : `Remove tag ${tag.name}`}>×</button>
        </li>)}
      </ul>
      {!loading && snapshot && assigned.length === 0 && <span className="empty-tag-label">No tags assigned</span>}
      {assigned.length > 6 && <button className="tag-show-all" onClick={() => setShowAll(value => !value)} aria-expanded={showAll}>{showAll ? 'Show fewer' : `+${assigned.length - 6} more`}</button>}
      <button ref={addButton} className="tag-add-button" disabled={disabled} onClick={openPicker} aria-expanded={pickerOpen} aria-controls={`${id}-picker`}>+ Add tags</button>
    </div>
    <div ref={picker} id={`${id}-picker`} popover="auto" className="video-tag-picker video-tag-editor" aria-label="Add tags" aria-busy={busy || loading} onToggle={event => { setPickerOpen(event.newState === 'open'); if (event.newState === 'closed') { setChosenTags([]); setTagQuery('') } }}>
      <header><strong>Add tags{batch ? ` to ${count} files` : ''}</strong><button type="button" onClick={closePicker} aria-label="Close tag picker">×</button></header>
      <form onSubmit={event => { event.preventDefault(); if (selectedTags.length) void mutate('add_video_tags', { tagIds: selectedTags }, batch ? `${selectedTags.length} tag(s) added to all ${count} selected files.` : `${selectedTags.length} tag(s) added.`) }}>
        <label className="tag-picker-legend" htmlFor={`${id}-existing`}>Find or create a tag</label>
        <div className="video-tag-input"><input ref={search} type="search" id={`${id}-existing`} placeholder="Find or create a tag…" value={tagQuery} onChange={event => setTagQuery(event.target.value)} disabled={disabled} /></div>
        <fieldset className="existing-tag-options" disabled={disabled}>
          <legend className="tag-picker-legend">Choose tags to add</legend>
          {matchingTags.map(tag => <label key={tag.id}><input type="checkbox" checked={selectedTags.includes(tag.id)} onChange={event => setChosenTags(previous => event.target.checked ? [...new Set([...previous, tag.id])] : previous.filter(tagId => tagId !== tag.id))} /><span>{tag.name}{batch && tag.assignedCount > 0 ? ` (${tag.assignedCount} of ${count})` : ''}</span></label>)}
          {!matchingTags.length && <p className="video-tag-help">{exactTag?.assignedCount === count ? 'This tag is already assigned.' : available.length ? 'No tags match your search.' : 'No other tags available. Type a name to create one.'}</p>}
        </fieldset>
        {queryName && !exactTag && <button type="button" className="tag-create-button" disabled={disabled} onClick={() => void mutate('create_and_assign_tag', { name: queryName }, addMessage)}>+ Create “{queryName}”{batch ? ' and add to all' : ' and add'}</button>}
        <div className="tag-picker-actions"><span>{selectedTags.length} selected</span><button type="button" disabled={disabled || !selectedTags.length} onClick={() => setChosenTags([])}>Clear</button><button type="submit" disabled={disabled || !selectedTags.length}>{batch ? 'Add to all' : 'Add selected'}</button></div>
      </form>
      {busy && <p className="video-tag-help" role="status">Saving tags…</p>}
      {error && <p className="video-tag-error" role="alert">{error}</p>}
    </div>
    <p className="video-tag-help tag-storage-note">Tags are saved locally.</p>
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
    <header><h2 id="batch-tags-title">Edit tags · {videos.length} {videos.length === 1 ? 'file' : 'files'}</h2><button onClick={onClose} disabled={busy} aria-label="Close tag editor">Close</button></header>
    <details className="tag-selected-videos"><summary>Selected files</summary><ul>{videos.map(video => <li key={video.videoId} title={video.path}>{video.name}</li>)}</ul></details>
    <VideoTags videos={videos.map(({ videoId, path }) => ({ videoId, path }))} onChanged={onChanged} onBusyChange={setBusy} />
    <footer><span>Changes are saved as you make them.</span><button onClick={onClose} disabled={busy}>Done</button></footer>
  </dialog>
}
