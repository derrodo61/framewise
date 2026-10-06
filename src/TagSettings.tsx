import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { ask } from '@tauri-apps/plugin-dialog'
import { error as logError } from '@tauri-apps/plugin-log'
import './tag-settings.css'

type Tag = { id: number; name: string; videoCount: number }

function report(cause: unknown) {
  const message = cause instanceof Error ? cause.message : String(cause)
  void logError(`Managing tags: ${message}`).catch(() => {})
  return message
}

export default function TagSettings() {
  const [tags, setTags] = useState<Tag[]>([])
  const [query, setQuery] = useState('')
  const [newName, setNewName] = useState('')
  const [editingId, setEditingId] = useState<number | null>(null)
  const [renameName, setRenameName] = useState('')
  const [revision, setRevision] = useState(0)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [notice, setNotice] = useState<string | null>(null)

  useEffect(() => {
    let active = true
    void invoke<Tag[]>('list_tags', { query })
      .then(result => { if (active) setTags(result) })
      .catch(cause => { if (active) setError(report(cause)) })
      .finally(() => { if (active) setLoading(false) })
    return () => { active = false }
  }, [query, revision])

  function refresh() { setLoading(true); setRevision(value => value + 1) }

  async function mutate(command: string, args: Record<string, unknown>, message: string) {
    setBusy(true); setError(null); setNotice(null)
    try { await invoke(command, args); setNotice(message); refresh(); return true }
    catch (cause) { setError(report(cause)); refresh(); return false }
    finally { setBusy(false) }
  }

  async function create() {
    if (!newName.trim() || busy || loading) return
    if (await mutate('create_tag', { name: newName }, `Created “${newName.trim()}”.`)) setNewName('')
  }
  async function rename(tag: Tag) {
    if (!renameName.trim() || busy || loading) return
    if (await mutate('rename_tag', { tagId: tag.id, name: renameName }, `Renamed “${tag.name}” to “${renameName.trim()}”.`)) setEditingId(null)
  }
  async function remove(tag: Tag) {
    if (busy || loading) return
    setBusy(true); setError(null); setNotice(null)
    try {
      const approved = await ask(`Delete tag “${tag.name}”? This removes the tag from ${tag.videoCount} ${tag.videoCount === 1 ? 'video' : 'videos'} across your collection, including missing or trashed videos. Your video files will not be deleted.`, { title: 'Delete tag', kind: 'warning', okLabel: 'Delete tag', cancelLabel: 'Cancel' })
      if (!approved) return
      await invoke('delete_tag', { tagId: tag.id, expectedName: tag.name, expectedVideoCount: tag.videoCount })
      setNotice(`Deleted tag “${tag.name}”.`); setEditingId(null); refresh()
    } catch (cause) { setError(report(cause)); refresh() }
    finally { setBusy(false) }
  }

  return <section className="settings-card tag-settings" aria-labelledby="manage-tags-heading">
    <div className="settings-card-heading"><div className="settings-card-icon" aria-hidden="true">#</div><div><h2 id="manage-tags-heading">Manage tags</h2><p>Create reusable labels to organize your videos. Tags are shared across your workspaces.</p></div></div>
    <form className="tag-create" onSubmit={event => { event.preventDefault(); void create() }}>
      <label htmlFor="new-tag-name">New tag</label>
      <div className="tag-input-actions"><input id="new-tag-name" placeholder="e.g. Landscape" value={newName} onChange={event => setNewName(event.target.value)} disabled={busy} aria-describedby="tag-name-help" /><button type="submit" disabled={busy || loading || !newName.trim()}>Create tag</button></div>
      <p id="tag-name-help" className="tag-help">Names can contain up to 100 characters. Names differing only by case count as the same tag.</p>
    </form>
    <div className="tag-search"><label htmlFor="tag-search">Search tags</label><div className="tag-input-actions"><input id="tag-search" type="search" placeholder="Find a tag…" value={query} onChange={event => { setQuery(event.target.value); setLoading(true); setError(null) }} disabled={busy} /><button type="button" onClick={() => { setError(null); refresh() }} disabled={busy || loading}>Refresh</button></div></div>
    {error && <div className="settings-error" role="alert">{error}</div>}
    {notice && <p className="tag-notice" role="status">{notice}</p>}
    {loading && <p className="settings-loading" role="status">Loading tags…</p>}
    <ul className="tag-list" aria-label="Tags" aria-busy={loading || busy}>
      {tags.map(tag => <li key={tag.id}>
        {editingId === tag.id ? <form className="tag-rename" onSubmit={event => { event.preventDefault(); void rename(tag) }} onKeyDown={event => { if (event.key === 'Escape' && !busy) setEditingId(null) }}><label className="tag-rename-label" htmlFor={`rename-tag-${tag.id}`}>Rename “{tag.name}”</label><div className="tag-input-actions"><input id={`rename-tag-${tag.id}`} autoFocus value={renameName} onChange={event => setRenameName(event.target.value)} disabled={busy} /><button type="submit" disabled={busy || loading || !renameName.trim()}>Save</button><button type="button" onClick={() => setEditingId(null)} disabled={busy}>Cancel</button></div></form>
          : <><div className="tag-summary"><strong>{tag.name}</strong><span>{tag.videoCount} {tag.videoCount === 1 ? 'video' : 'videos'}</span></div><div className="tag-row-actions"><button onClick={() => { setEditingId(tag.id); setRenameName(tag.name); setError(null); setNotice(null) }} disabled={busy || loading} aria-label={`Rename tag ${tag.name}`}>Rename</button><button className="tag-delete" onClick={() => void remove(tag)} disabled={busy || loading} aria-label={`Delete tag ${tag.name}`}>Delete</button></div></>}
      </li>)}
    </ul>
    {!loading && !error && tags.length === 0 && <p className="tag-empty">{query.trim() ? 'No tags match your search.' : 'No tags yet. Create your first tag above.'}</p>}
    <p className="settings-footnote">Renaming a tag updates its name on every assigned video. Deleting a tag removes its assignments, without deleting videos.</p>
  </section>
}
