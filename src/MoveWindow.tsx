import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { open } from '@tauri-apps/plugin-dialog'
import { displayPath } from './paths'
import './move-window.css'

type FileEntry = { name: string; path: string; isDirectory: boolean }
type DirectoryListing = { path: string; parent: string | null; entries: FileEntry[] }
type MoveSession = { sourceFolder: string; names: string[] }
type MoveResult = { count: number; destination: string }

function errorText(cause: unknown) { return cause instanceof Error ? cause.message : String(cause) }

export default function MoveWindow() {
  const [session, setSession] = useState<MoveSession | null>(null)
  const [listing, setListing] = useState<DirectoryListing | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [moved, setMoved] = useState(false)
  const [newFolder, setNewFolder] = useState('')
  const [creatingFolder, setCreatingFolder] = useState(false)

  useEffect(() => {
    let active = true
    invoke<MoveSession>('move_session')
      .then(async value => {
        const directory = await invoke<DirectoryListing>('list_move_directory', { path: value.sourceFolder })
        if (active) { setSession(value); setListing(directory) }
      })
      .catch(cause => { if (active) setError(errorText(cause)) })
    return () => { active = false }
  }, [])

  async function browse(path: string) {
    if (busy) return
    setError(null)
    try {
      setListing(await invoke<DirectoryListing>('list_move_directory', { path }))
      setCreatingFolder(false); setNewFolder('')
    }
    catch (cause) { setError(errorText(cause)) }
  }

  async function chooseFolder() {
    if (busy) return
    try {
      const path = await open({ directory: true, multiple: false, title: 'Choose destination folder', defaultPath: listing?.path })
      if (path && !Array.isArray(path)) await browse(path)
    } catch (cause) { setError(errorText(cause)) }
  }

  async function createFolder() {
    if (!listing || !newFolder || busy) return
    setBusy(true); setError(null)
    try {
      const directory = await invoke<DirectoryListing>('create_move_folder', { parent: listing.path, name: newFolder })
      setListing(directory); setNewFolder(''); setCreatingFolder(false)
    } catch (cause) { setError(errorText(cause)) }
    finally { setBusy(false) }
  }

  async function moveHere() {
    if (!listing || !session || busy || moved) return
    setBusy(true); setError(null)
    try {
      const result = await invoke<MoveResult>('move_selected', { destination: listing.path })
      setMoved(true)
      try { await getCurrentWindow().close() }
      catch { setError(`Moved ${result.count} ${result.count === 1 ? 'video' : 'videos'}. You can close this window now.`) }
    } catch (cause) { setError(errorText(cause)) }
    finally { setBusy(false) }
  }

  const folders = listing?.entries.filter(entry => entry.isDirectory) ?? []
  const videos = listing?.entries.filter(entry => !entry.isDirectory) ?? []
  const sameFolder = listing?.path === session?.sourceFolder

  return <div className="move-window">
    <header className="move-header"><span className="move-eyebrow">FRAMEWISE</span><h1>Move to</h1><p>{session ? `${session.names.length} ${session.names.length === 1 ? 'video' : 'videos'} selected` : 'Preparing files…'}</p></header>
    {session && <details className="move-selection"><summary>Selected files</summary><ul>{session.names.map(name => <li key={name}>{name}</li>)}</ul></details>}
    <div className="move-location"><span>DESTINATION</span><strong title={listing ? displayPath(listing.path) : undefined}>{listing ? displayPath(listing.path) : 'Loading folder…'}</strong><button onClick={() => void chooseFolder()} disabled={busy}>Choose another folder…</button></div>
    {error && <div className="move-error" role="alert">{error}</div>}
    <div className="move-list-toolbar">
      {creatingFolder
        ? <form className="move-create" onSubmit={event => { event.preventDefault(); void createFolder() }}><input autoFocus aria-label="New folder name" placeholder="New folder name" value={newFolder} onChange={event => setNewFolder(event.target.value)} disabled={busy} /><button type="submit" disabled={busy || !newFolder.trim()}>Create</button><button type="button" onClick={() => { setCreatingFolder(false); setNewFolder('') }} disabled={busy}>Cancel folder</button></form>
        : <button className="move-new-folder" onClick={() => setCreatingFolder(true)} disabled={!listing || busy || moved}>+ New folder</button>}
      <div className="move-actions"><button onClick={() => void getCurrentWindow().close()} disabled={busy}>{moved ? 'Close' : 'Cancel'}</button><button className="move-primary" onClick={() => void moveHere()} disabled={!listing || !session || sameFolder || busy || moved || creatingFolder}>{busy ? 'Moving…' : 'Move here'}</button></div>
    </div>
    <main className="move-folder-list" aria-label="Destination folders">
      {listing?.parent && <button className="move-folder" onClick={() => void browse(listing.parent!)} disabled={busy}><span>↑</span> One folder up</button>}
      {folders.map(folder => <button className="move-folder" key={folder.path} onClick={() => void browse(folder.path)} disabled={busy}><span>▣</span> {folder.name}</button>)}
      {videos.map(video => <div className="move-file" key={video.path}><span>▤</span> {video.name}</div>)}
      {listing && folders.length === 0 && videos.length === 0 && <p className="move-empty">This folder is empty.</p>}
    </main>
    <footer className="move-footer">
      <button className="move-new-folder" onClick={() => setCreatingFolder(true)} disabled={!listing || busy || moved || creatingFolder}>+ New folder</button>
      <div className="move-actions"><button onClick={() => void getCurrentWindow().close()} disabled={busy}>{moved ? 'Close' : 'Cancel'}</button><button className="move-primary" onClick={() => void moveHere()} disabled={!listing || !session || sameFolder || busy || moved || creatingFolder}>{busy ? 'Moving…' : 'Move here'}</button></div>
    </footer>
  </div>
}
