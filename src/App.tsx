import { useEffect, useEffectEvent, useMemo, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import type { CSSProperties, KeyboardEvent, MouseEvent, PointerEvent } from 'react'
import { convertFileSrc, invoke, isTauri } from '@tauri-apps/api/core'
import { ask, open } from '@tauri-apps/plugin-dialog'
import { error as logError, info as logInfo } from '@tauri-apps/plugin-log'
import { listen } from '@tauri-apps/api/event'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import Editor from './Editor'
import GenerationPrompt from './GenerationPrompt'
import MediaThumbnail from './MediaThumbnail'
import { generateThumbnail } from './thumbnailQueue'
import { nextMediaIndex } from './mediaNavigation'
import { sortMedia } from './mediaSort'
import type { MediaSort, SortDirection } from './mediaSort'
import type { EditResult } from './Editor'
import { displayPath, versionedMediaSrc } from './paths'
import { getPreference, setPreference, removePreference } from './preferences'
import './App.css'
import './panels.css'
import './settings.css'
import './navigation.css'
import './theme.css'
import './preview.css'
import './file-actions.css'
import './media-view.css'

type FileEntry = { name: string; path: string; isDirectory: boolean; size: number | null; modifiedAt: number | null }
type DirectoryListing = { path: string; parent: string | null; entries: FileEntry[] }
type TrashBatchResult = { listing: DirectoryListing; movedCount: number; error: string | null }
type DuplicateResult = { listing: DirectoryListing; duplicatedPath: string }
type RenameResult = { listing: DirectoryListing; renamedPath: string }
type CreatedFolder = { listing: DirectoryListing; createdPath: string }
type MoveResult = { count: number; sourceFolder: string; destination: string; directories: boolean }
type ProbeStream = {
  index?: number; codec_type?: string; codec_name?: string; profile?: string
  width?: number; height?: number; r_frame_rate?: string; avg_frame_rate?: string
  bit_rate?: string; sample_rate?: string; channels?: number; channel_layout?: string
  pix_fmt?: string; color_space?: string; color_transfer?: string; color_primaries?: string
  tags?: Record<string, string>
}
type Probe = {
  format?: { format_name?: string; format_long_name?: string; duration?: string; size?: string; bit_rate?: string; tags?: Record<string, string> }
  streams?: ProbeStream[]
  chapters?: unknown[]
}
type Preview = { videoPath: string; videoVersion: string; thumbnailPath: string | null }

function editableName(file: FileEntry) { return file.isDirectory ? file.name : file.name.slice(0, file.name.lastIndexOf('.')) }
function fixedExtension(file: FileEntry) { return file.isDirectory ? '' : file.name.slice(file.name.lastIndexOf('.')) }

function Icon({ name, size = 20 }: { name: 'folder' | 'film' | 'chevron' | 'arrow' | 'info' | 'refresh' | 'copy' | 'check' | 'close' | 'settings' | 'sun' | 'moon' | 'play'; size?: number }) {
  const paths: Record<typeof name, React.ReactNode> = {
    folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />,
    film: <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M7 4v16M17 4v16M3 9h4m-4 6h4m10-6h4m-4 6h4" /></>,
    chevron: <path d="m9 18 6-6-6-6" />,
    arrow: <path d="m12 19-7-7 7-7m-7 7h14" />,
    info: <><circle cx="12" cy="12" r="9" /><path d="M12 11v5m0-8h.01" /></>,
    refresh: <><path d="M20 11a8 8 0 1 0-2 6" /><path d="M20 5v6h-6" /></>,
    copy: <><rect x="8" y="8" width="12" height="12" rx="2" /><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" /></>,
    check: <path d="m5 12 4 4L19 6" />,
    close: <path d="M6 6l12 12M18 6 6 18" />,
    settings: <><path d="M4 7h16M4 12h16M4 17h16" /><circle cx="9" cy="7" r="2" fill="white" /><circle cx="16" cy="12" r="2" fill="white" /><circle cx="10" cy="17" r="2" fill="white" /></>,
    sun: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2m0 16v2M4.93 4.93l1.42 1.42m11.3 11.3 1.42 1.42M2 12h2m16 0h2M4.93 19.07l1.42-1.42m11.3-11.3 1.42-1.42" /></>,
    moon: <path d="M20.5 14.5A8.5 8.5 0 0 1 9.5 3.5 8.5 8.5 0 1 0 20.5 14.5Z" />,
    play: <path d="m8 5 11 7-11 7z" fill="currentColor" stroke="none" />,
  }
  return <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{paths[name]}</svg>
}

function fileSize(bytes?: number | string | null) {
  if (bytes === undefined || bytes === null || bytes === '') return '—'
  const value = Number(bytes)
  if (!Number.isFinite(value) || value < 0) return '—'
  if (value < 1024) return `${value} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  const index = Math.min(Math.floor(Math.log(value) / Math.log(1024)) - 1, units.length - 1)
  return `${(value / 1024 ** (index + 1)).toFixed(value / 1024 ** (index + 1) >= 10 ? 0 : 1)} ${units[index]}`
}

function duration(seconds?: string) {
  const value = Number(seconds)
  if (!Number.isFinite(value)) return '—'
  const whole = Math.floor(value)
  const hours = Math.floor(whole / 3600)
  const minutes = Math.floor((whole % 3600) / 60)
  const secs = whole % 60
  return hours ? `${hours}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}` : `${minutes}:${String(secs).padStart(2, '0')}`
}

function frameRate(rate?: string) {
  if (!rate) return '—'
  const [numerator, denominator] = rate.split('/').map(Number)
  const value = numerator / (denominator || 1)
  return Number.isFinite(value) && value > 0 ? `${Number(value.toFixed(2))} fps` : '—'
}

function bitrate(rate?: string) {
  const value = Number(rate)
  return Number.isFinite(value) && value > 0 ? `${(value / 1_000_000).toFixed(2)} Mb/s` : '—'
}

function errorText(error: unknown) { return error instanceof Error ? error.message : String(error) }
function reportError(context: string, cause: unknown) {
  const message = errorText(cause)
  if (isTauri()) void logError(`${context}: ${message}`).catch(() => {})
  return message
}
function display(value: unknown) { return value === undefined || value === null || value === '' ? '—' : String(value) }
function savedPanelState(key: 'framewise.workspaceCollapsed' | 'framewise.inspectorCollapsed') { return getPreference(key) === 'true' }
function savedPanelWidth(key: 'framewise.workspaceWidth' | 'framewise.inspectorWidth', fallback: number) {
  const saved = getPreference(key)
  const width = saved === null ? NaN : Number(saved)
  return Number.isFinite(width) && width > 0 ? width : fallback
}

const MIN_MEDIA_WIDTH = 320
const MIN_WORKSPACE_WIDTH = 140
const MIN_INSPECTOR_WIDTH = 240
const COLLAPSED_WIDTH = 56
type PanelSide = 'left' | 'right'

function Property({ label, value }: { label: string; value: unknown }) {
  return <div className="property"><dt>{label}</dt><dd>{display(value)}</dd></div>
}

function VideoPreview({ file, playRequested, onPlayRequestHandled }: { file: FileEntry; playRequested: boolean; onPlayRequestHandled: () => void }) {
  const videoRef = useRef<HTMLVideoElement | null>(null)
  const lookupStartedAt = useRef(0)
  const [preview, setPreview] = useState<Preview | null>(null)
  const [previewError, setPreviewError] = useState<string | null>(null)
  const [thumbnailPending, setThumbnailPending] = useState(false)
  const [playing, setPlaying] = useState(false)
  const [playbackError, setPlaybackError] = useState(false)

  useEffect(() => {
    let active = true
    let timer: number | undefined
    lookupStartedAt.current = performance.now()
    invoke<Preview>('prepare_preview', { path: file.path })
      .then(result => {
        if (!active) return
        const elapsed = Math.round(performance.now() - lookupStartedAt.current)
        if (elapsed > 250) void logInfo(`Preview request took ${elapsed} ms (cached: ${Boolean(result.thumbnailPath)}): ${file.name}`)
        setPreview(result)
        if (result.thumbnailPath) return
        setThumbnailPending(true)
        timer = window.setTimeout(() => {
          generateThumbnail(file.path, () => active, true)
            .then(thumbnailPath => {
              if (active && thumbnailPath) setPreview(current => current ? { ...current, thumbnailPath } : current)
            })
            .catch(cause => { if (active) setPreviewError(reportError('Preparing video thumbnail', cause)) })
            .finally(() => { if (active) setThumbnailPending(false) })
        }, 120)
      })
      .catch(cause => { if (active) setPreviewError(reportError('Preparing video preview', cause)) })
    return () => { active = false; window.clearTimeout(timer) }
  }, [file.path, file.name])

  const videoUrl = preview ? versionedMediaSrc(preview.videoPath, preview.videoVersion) : null
  const thumbnailUrl = preview?.thumbnailPath ? convertFileSrc(preview.thumbnailPath) : null

  useEffect(() => {
    const video = videoRef.current
    return () => { video?.pause(); video?.removeAttribute('src') }
  }, [videoUrl])

  function startPlayback() {
    if (!videoRef.current) return
    setPlaybackError(false)
    setPlaying(true)
    void videoRef.current.play().catch(() => { setPlaying(false); setPlaybackError(true) })
  }

  const playFromList = useEffectEvent(() => {
    startPlayback()
    onPlayRequestHandled()
  })
  useEffect(() => {
    if (playRequested && videoUrl) playFromList()
  }, [playRequested, videoUrl])

  return <section className="preview-section" aria-label="Video preview">
    <div className="section-title preview-title">PREVIEW</div>
    <div className={`preview-frame ${playing ? 'is-playing' : ''}`}>
      {videoUrl && <video ref={videoRef} src={videoUrl} poster={thumbnailUrl ?? undefined} preload="none" controls={playing} playsInline onError={() => { setPlaying(false); setPlaybackError(true) }} aria-label={`Playing ${file.name}`} />}
      {!playing && (thumbnailUrl ? <img src={thumbnailUrl} alt={`Preview frame from ${file.name}`} onLoad={() => {
        const elapsed = Math.round(performance.now() - lookupStartedAt.current)
        if (elapsed > 250) void logInfo(`Preview image displayed after ${elapsed} ms: ${file.name}`)
      }} /> : <div className="preview-placeholder"><Icon name="film" size={36} /></div>)}
      {preview && !playing && <button className="preview-play" onClick={startPlayback} aria-label={`Play ${file.name}`}><Icon name="play" size={24} /></button>}
      {(!preview || (thumbnailPending && !playing)) && !previewError && <span className="preview-loading">Preparing preview…</span>}
    </div>
    {previewError && <p className="preview-message" role="alert">Preview unavailable: {previewError}</p>}
    {playbackError && <p className="preview-message" role="alert">This video format may not play in the system WebView. The metadata remains available below.</p>}
  </section>
}

function App() {
  const [mediaView, setMediaView] = useState<'list' | 'grid'>(() => getPreference('framewise.mediaView') === 'grid' ? 'grid' : 'list')
  const [gridSize, setGridSize] = useState<'small' | 'medium' | 'large'>(() => {
    const saved = getPreference('framewise.gridSize')
    return saved === 'small' || saved === 'large' ? saved : 'medium'
  })
  const [mediaSort, setMediaSort] = useState<MediaSort>(() => getPreference('framewise.mediaSort') === 'modified' ? 'modified' : 'name')
  const [sortDirection, setSortDirection] = useState<SortDirection>(() => getPreference('framewise.sortDirection') === 'desc' ? 'desc' : 'asc')
  const [root, setRoot] = useState<string | null>(null)
  const [listing, setListing] = useState<DirectoryListing | null>(null)
  const mediaEntries = useMemo(() => sortMedia(listing?.entries ?? [], mediaSort, sortDirection), [listing, mediaSort, sortDirection])
  const [selected, setSelected] = useState<FileEntry | null>(null)
  const [playbackPath, setPlaybackPath] = useState<string | null>(null)
  const [selectedPaths, setSelectedPaths] = useState<string[]>([])
  const [probe, setProbe] = useState<Probe | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [copyDone, setCopyDone] = useState(false)
  const [editingFile, setEditingFile] = useState<FileEntry | null>(null)
  const [contextMenu, setContextMenu] = useState<{ file: FileEntry; x: number; y: number } | null>(null)
  const [fileAction, setFileAction] = useState<{ message: string; error: boolean } | null>(null)
  const [deleting, setDeleting] = useState(false)
  const trashRequestPending = useRef(false)
  const [duplicatingFile, setDuplicatingFile] = useState<string | null>(null)
  const [renameTarget, setRenameTarget] = useState<FileEntry | null>(null)
  const [renameStem, setRenameStem] = useState('')
  const [renameError, setRenameError] = useState<string | null>(null)
  const [renaming, setRenaming] = useState(false)
  const [creatingFolder, setCreatingFolder] = useState(false)
  const [newFolderName, setNewFolderName] = useState('')
  const [folderBusy, setFolderBusy] = useState(false)
  const [view, setView] = useState<'media' | 'settings'>('media')
  const [theme, setTheme] = useState<'light' | 'dark'>(() => document.documentElement.dataset.theme === 'dark' ? 'dark' : 'light')
  const [defaultFolder, setDefaultFolder] = useState<string | null>(() => getPreference('framewise.defaultFolder'))
  const [settingsError, setSettingsError] = useState<string | null>(null)
  const [previewCacheDirectory, setPreviewCacheDirectory] = useState<string | null>(null)
  const [previewIndexLocation, setPreviewIndexLocation] = useState<string | null>(null)
  const [previewCacheError, setPreviewCacheError] = useState<string | null>(null)
  const [previewIndexError, setPreviewIndexError] = useState<string | null>(null)
  const [startupLoading, setStartupLoading] = useState(() => Boolean(getPreference('framewise.defaultFolder')))
  const [workspaceCollapsed, setWorkspaceCollapsed] = useState(() => savedPanelState('framewise.workspaceCollapsed'))
  const [inspectorCollapsed, setInspectorCollapsed] = useState(() => savedPanelState('framewise.inspectorCollapsed'))
  const [viewportWidth, setViewportWidth] = useState(window.innerWidth)
  const [workspaceWidth, setWorkspaceWidth] = useState(() => savedPanelWidth('framewise.workspaceWidth', window.innerWidth <= 800 ? 150 : window.innerWidth <= 1050 ? 180 : 230))
  const [inspectorWidth, setInspectorWidth] = useState(() => savedPanelWidth('framewise.inspectorWidth', window.innerWidth <= 800 ? 260 : window.innerWidth <= 1050 ? 300 : 350))
  const [resizing, setResizing] = useState<PanelSide | null>(null)
  const drag = useRef<{ side: PanelSide; startX: number; startWidth: number; lastWidth: number; limits: { min: number; max: number } } | null>(null)
  const requestId = useRef(0)
  const rootQueue = useRef<Promise<void>>(Promise.resolve())
  const fileListRef = useRef<HTMLDivElement | null>(null)
  const contextMenuRef = useRef<HTMLDivElement | null>(null)
  const pendingFocus = useRef<{ path?: string } | null>(null)
  const selectionAnchor = useRef<string | null>(null)
  const listingRef = useRef<DirectoryListing | null>(null)

  function selectRoot(path: string) {
    const next = rootQueue.current.then(() => invoke<DirectoryListing>('select_root', { path }))
    rootQueue.current = next.then(() => {}, () => {})
    return next
  }

  useEffect(() => {
    const onResize = () => setViewportWidth(window.innerWidth)
    window.addEventListener('resize', onResize)
    return () => window.removeEventListener('resize', onResize)
  }, [])

  useEffect(() => {
    if (view !== 'settings' || !isTauri()) return
    let active = true
    invoke<string>('preview_cache_directory')
      .then(path => { if (active) { setPreviewCacheDirectory(path); setPreviewCacheError(null) } })
      .catch(cause => { if (active) setPreviewCacheError(reportError('Locating preview cache', cause)) })
    invoke<string>('preview_index_location')
      .then(path => { if (active) { setPreviewIndexLocation(path); setPreviewIndexError(null) } })
      .catch(cause => { if (active) setPreviewIndexError(reportError('Locating preview index', cause)) })
    return () => { active = false }
  }, [view])

  useEffect(() => {
    const saved = getPreference('framewise.defaultFolder')
    if (!saved) return
    let active = true
    const currentRequest = ++requestId.current
    selectRoot(saved)
      .then(next => {
        if (!active || currentRequest !== requestId.current) return
        setRoot(next.path); setListing(next); setSelected(null); setSelectedPaths([]); selectionAnchor.current = null; setProbe(null)
        if (next.path !== saved) {
          setPreference('framewise.defaultFolder', next.path)
          setDefaultFolder(next.path)
        }
      })
      .catch(cause => {
        if (!active || currentRequest !== requestId.current) return
        setSettingsError(`Could not open the startup folder: ${reportError('Opening startup folder', cause)}`)
        setView('settings')
      })
      .finally(() => { if (active && currentRequest === requestId.current) setStartupLoading(false) })
    return () => { active = false }
  }, [])

  useEffect(() => {
    if (view !== 'media' || !listing || !pendingFocus.current) return
    const rows = Array.from(fileListRef.current?.querySelectorAll<HTMLButtonElement>('[data-list-row]') ?? [])
    const target = pendingFocus.current.path
      ? rows.find(row => row.dataset.path === pendingFocus.current?.path)
      : rows.find(row => row.dataset.entryIndex === '0') ?? rows[0]
    pendingFocus.current = null
    target?.focus()
  }, [listing, view])

  useEffect(() => {
    if (!contextMenu) return
    contextMenuRef.current?.querySelector('button')?.focus()
    const closeOnPointer = (event: globalThis.PointerEvent) => {
      if (!contextMenuRef.current?.contains(event.target as Node)) setContextMenu(null)
    }
    const closeOnKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') {
        setContextMenu(null)
        Array.from(fileListRef.current?.querySelectorAll<HTMLButtonElement>('[data-list-row]') ?? [])
          .find(row => row.dataset.path === contextMenu.file.path)?.focus()
      }
    }
    const close = () => setContextMenu(null)
    document.addEventListener('pointerdown', closeOnPointer)
    document.addEventListener('keydown', closeOnKey)
    window.addEventListener('resize', close)
    window.addEventListener('scroll', close, true)
    return () => {
      document.removeEventListener('pointerdown', closeOnPointer)
      document.removeEventListener('keydown', closeOnKey)
      window.removeEventListener('resize', close)
      window.removeEventListener('scroll', close, true)
    }
  }, [contextMenu])

  const availableForPanels = Math.max(viewportWidth, 760) - MIN_MEDIA_WIDTH
  const effectiveWorkspaceWidth = workspaceCollapsed
    ? COLLAPSED_WIDTH
    : Math.max(MIN_WORKSPACE_WIDTH, Math.min(workspaceWidth, availableForPanels - (inspectorCollapsed ? COLLAPSED_WIDTH : MIN_INSPECTOR_WIDTH)))
  const effectiveInspectorWidth = inspectorCollapsed
    ? COLLAPSED_WIDTH
    : Math.max(MIN_INSPECTOR_WIDTH, Math.min(inspectorWidth, availableForPanels - effectiveWorkspaceWidth))

  function widthLimits(side: PanelSide) {
    return side === 'left'
      ? { min: MIN_WORKSPACE_WIDTH, max: availableForPanels - effectiveInspectorWidth }
      : { min: MIN_INSPECTOR_WIDTH, max: availableForPanels - effectiveWorkspaceWidth }
  }

  function setPanelWidth(side: PanelSide, nextWidth: number, save: boolean, limits = widthLimits(side)) {
    const { min, max } = limits
    const width = Math.round(Math.max(min, Math.min(nextWidth, max)))
    if (side === 'left') setWorkspaceWidth(width)
    else setInspectorWidth(width)
    if (save) setPreference(side === 'left' ? 'framewise.workspaceWidth' : 'framewise.inspectorWidth', String(width))
    return width
  }

  function startResize(side: PanelSide, event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    const width = side === 'left' ? effectiveWorkspaceWidth : effectiveInspectorWidth
    drag.current = { side, startX: event.clientX, startWidth: width, lastWidth: width, limits: widthLimits(side) }
    setResizing(side)
  }

  function moveResize(event: PointerEvent<HTMLDivElement>) {
    if (!drag.current) return
    const { side, startX, startWidth, limits } = drag.current
    const delta = event.clientX - startX
    drag.current.lastWidth = setPanelWidth(side, startWidth + (side === 'left' ? delta : -delta), false, limits)
  }

  function endResize(event: PointerEvent<HTMLDivElement>) {
    if (!drag.current) return
    const { side, lastWidth } = drag.current
    setPreference(side === 'left' ? 'framewise.workspaceWidth' : 'framewise.inspectorWidth', String(lastWidth))
    drag.current = null
    setResizing(null)
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
  }

  function resizeWithKeyboard(side: PanelSide, event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    event.preventDefault()
    const direction = event.key === 'ArrowRight' ? 1 : -1
    const current = side === 'left' ? effectiveWorkspaceWidth : effectiveInspectorWidth
    setPanelWidth(side, current + direction * (side === 'left' ? 1 : -1) * (event.shiftKey ? 32 : 10), true)
  }

  function toggleWorkspace() {
    setWorkspaceCollapsed(current => {
      setPreference('framewise.workspaceCollapsed', String(!current))
      return !current
    })
  }

  function toggleInspector() {
    setInspectorCollapsed(current => {
      setPreference('framewise.inspectorCollapsed', String(!current))
      return !current
    })
  }

  async function chooseFolder() {
    try {
      const path = await open({ directory: true, multiple: false, title: 'Choose a video folder' })
      if (!path || Array.isArray(path)) return
      const currentRequest = ++requestId.current
      const next = await selectRoot(path)
      if (currentRequest !== requestId.current) return
      pendingFocus.current = {}
      setRoot(next.path); setListing(next); setSelected(null); setSelectedPaths([]); selectionAnchor.current = null; setProbe(null); setError(null); setView('media'); setStartupLoading(false); setCreatingFolder(false); setNewFolderName('')
      inspectFocusedVideo(next, {})
    } catch (cause) { setError(reportError('Choosing media folder', cause)) }
  }

  async function chooseDefaultFolder() {
    try {
      const path = await open({ directory: true, multiple: false, title: 'Choose startup folder' })
      if (!path || Array.isArray(path)) return
      const currentRequest = ++requestId.current
      const next = await selectRoot(path)
      if (currentRequest !== requestId.current) return
      setPreference('framewise.defaultFolder', next.path)
      setDefaultFolder(next.path); setSettingsError(null); setStartupLoading(false)
      setRoot(next.path); setListing(next); setSelected(null); setSelectedPaths([]); selectionAnchor.current = null; setProbe(null); setError(null); setCreatingFolder(false); setNewFolderName('')
    } catch (cause) { setSettingsError(reportError('Choosing startup folder', cause)) }
  }

  function useCurrentAsDefault() {
    if (!root) return
    requestId.current++
    setPreference('framewise.defaultFolder', root)
    setDefaultFolder(root); setSettingsError(null); setStartupLoading(false)
  }

  function clearDefaultFolder() {
    requestId.current++
    removePreference('framewise.defaultFolder')
    setDefaultFolder(null); setSettingsError(null); setStartupLoading(false)
  }

  function chooseTheme(nextTheme: 'light' | 'dark') {
    document.documentElement.dataset.theme = nextTheme
    setPreference('framewise.theme', nextTheme)
    setTheme(nextTheme)
  }

  async function browse(path: string, focusTarget?: { path?: string }) {
    try {
      setPlaybackPath(null)
      setContextMenu(null); setFileAction(null)
      const currentRequest = ++requestId.current
      const next = await invoke<DirectoryListing>('list_directory', { path })
      if (currentRequest !== requestId.current) return
      pendingFocus.current = focusTarget ?? null
      setListing(next); setSelected(null); setSelectedPaths([]); selectionAnchor.current = null; setProbe(null); setError(null); setLoading(false); setView('media'); setCreatingFolder(false); setNewFolderName('')
      if (focusTarget) inspectFocusedVideo(next, focusTarget)
    } catch (cause) { setError(reportError('Browsing folder', cause)) }
  }

  function inspectFocusedVideo(directory: DirectoryListing, focusTarget: { path?: string }) {
    const entry = focusTarget.path
      ? directory.entries.find(item => item.path === focusTarget.path)
      : sortMedia(directory.entries, mediaSort, sortDirection)[0]
    if (entry && !entry.isDirectory) void selectFile(entry)
  }

  async function selectFile(file: FileEntry, delayMs = 0, preserveMulti = false) {
    setPlaybackPath(null)
    const currentRequest = ++requestId.current
    if (!preserveMulti) setSelectedPaths([file.path])
    setSelected(file); setProbe(null); setError(null); setLoading(true)
    try {
      if (delayMs) {
        await new Promise(resolve => window.setTimeout(resolve, delayMs))
        if (currentRequest !== requestId.current) return
      }
      const result = await invoke<Probe>('inspect_video', { path: file.path })
      if (currentRequest === requestId.current) setProbe(result)
    } catch (cause) {
      if (currentRequest === requestId.current) setError(reportError('Inspecting video', cause))
    } finally {
      if (currentRequest === requestId.current) setLoading(false)
    }
  }

  async function createMediaFolder() {
    if (!listing || !newFolderName.trim() || folderBusy) return
    setFolderBusy(true)
    setFileAction(null)
    const currentRequest = ++requestId.current
    try {
      const result = await invoke<CreatedFolder>('create_media_folder', { parent: listing.path, name: newFolderName })
      if (currentRequest !== requestId.current) return
      pendingFocus.current = { path: result.createdPath }
      setListing(result.listing)
      setSelected(null); setSelectedPaths([]); selectionAnchor.current = null; setProbe(null); setError(null); setLoading(false)
      setCreatingFolder(false); setNewFolderName('')
      setFileAction({ message: `Created folder “${result.listing.entries.find(entry => entry.path === result.createdPath)?.name ?? newFolderName}”.`, error: false })
    } catch (cause) { setFileAction({ message: reportError('Creating folder', cause), error: true }) }
    finally { setFolderBusy(false) }
  }

  useEffect(() => { listingRef.current = listing }, [listing])

  const refreshMovedFiles = useEffectEvent(({ count, sourceFolder, destination, directories }: MoveResult) => {
    const noun = directories ? (count === 1 ? 'folder' : 'folders') : (count === 1 ? 'video' : 'videos')
    const message = `Moved ${count} ${noun} to ${displayPath(destination)}.`
    const current = listingRef.current
    if (current && (current.path === sourceFolder || current.path === destination)) {
      void browse(current.path).then(() => setFileAction({ message, error: false }))
    } else setFileAction({ message, error: false })
  })

  useEffect(() => {
    let active = true
    let unlisten: (() => void) | null = null
    void listen<MoveResult>('files-moved', event => {
      if (!active) return
      refreshMovedFiles(event.payload)
    }).then(stop => { if (active) unlisten = stop; else stop() })
      .catch(cause => reportError('Listening for moved items', cause))
    return () => { active = false; unlisten?.() }
  }, [])

  function openFileMenu(file: FileEntry, x: number, y: number) {
    if (deleting || duplicatingFile || renaming || renameTarget) return
    if (!selectedPaths.includes(file.path)) { selectionAnchor.current = file.path; inspectSelection(file) }
    setContextMenu({ file, x: Math.max(8, Math.min(x, window.innerWidth - 200)), y: Math.max(8, Math.min(y, window.innerHeight - (file.isDirectory ? 180 : 255))) })
  }

  async function showInFileManager(file: FileEntry) {
    setContextMenu(null)
    try { await invoke('reveal_in_file_manager', { path: file.path }) }
    catch (cause) { setFileAction({ message: reportError('Showing item in file manager', cause), error: true }) }
  }

  function inspectSelection(entry: FileEntry | undefined, preserveMulti = false) {
    if (entry && !entry.isDirectory) { void selectFile(entry, 0, preserveMulti); return }
    requestId.current++
    if (!preserveMulti) setSelectedPaths(entry ? [entry.path] : [])
    setSelected(null); setProbe(null); setError(null); setLoading(false)
  }

  function selectMediaEntry(entry: FileEntry, index: number, modifiers: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) {
    const additive = modifiers.ctrlKey || modifiers.metaKey
    if (entry.isDirectory && !additive && !modifiers.shiftKey) { void browse(entry.path); return }
    const compatiblePaths = selectedPaths.filter(path => mediaEntries.some(item => item.path === path && item.isDirectory === entry.isDirectory))
    if (modifiers.shiftKey && listing) {
      const anchor = mediaEntries.findIndex(item => item.path === selectionAnchor.current && item.isDirectory === entry.isDirectory)
      const from = anchor < 0 ? index : anchor
      const range = mediaEntries.slice(Math.min(from, index), Math.max(from, index) + 1).filter(item => item.isDirectory === entry.isDirectory).map(item => item.path)
      if (anchor < 0) selectionAnchor.current = entry.path
      setSelectedPaths(additive ? [...new Set([...compatiblePaths, ...range])] : range)
      inspectSelection(entry, true)
    } else if (additive) {
      selectionAnchor.current = entry.path
      const next = compatiblePaths.includes(entry.path) ? compatiblePaths.filter(path => path !== entry.path) : [...compatiblePaths, entry.path]
      setSelectedPaths(next)
      inspectSelection(next.includes(entry.path) ? entry : mediaEntries.find(item => item.path === next.at(-1)), true)
    } else {
      selectionAnchor.current = entry.path
      void selectFile(entry)
    }
  }

  function playMediaEntry(entry: FileEntry) {
    if (entry.isDirectory) return
    // Select explicitly so modifier-clicks cannot leave a different video in the Inspector.
    selectionAnchor.current = entry.path
    if (selected?.path !== entry.path) void selectFile(entry)
    else setSelectedPaths([entry.path])
    if (inspectorCollapsed) {
      setInspectorCollapsed(false)
      setPreference('framewise.inspectorCollapsed', 'false')
    }
    setPlaybackPath(entry.path)
  }

  async function openMoveWindow(file: FileEntry) {
    setContextMenu(null)
    const paths = selectedPaths.includes(file.path) ? selectedPaths : [file.path]
    try {
      const existing = await WebviewWindow.getByLabel('move-to')
      if (existing) { await existing.setFocus(); return }
      await invoke('begin_move', { paths })
      const moveWindow = new WebviewWindow('move-to', { url: 'index.html?moveTo=1', title: 'Move to — Framewise', width: 650, height: 680, minWidth: 480, minHeight: 500 })
      await moveWindow.once('tauri://error', event => setFileAction({ message: `Could not open Move To window: ${String(event.payload)}`, error: true }))
    } catch (cause) { setFileAction({ message: reportError('Opening Move To window', cause), error: true }) }
  }

  function editFile(file: FileEntry) {
    setContextMenu(null)
    setEditingFile(file)
  }

  function fileContextMenu(event: MouseEvent<HTMLButtonElement>, file: FileEntry) {
    event.preventDefault()
    openFileMenu(file, event.clientX, event.clientY)
  }

  async function duplicateFile(file: FileEntry) {
    setContextMenu(null)
    setFileAction(null)
    setDuplicatingFile(file.name)
    const currentRequest = ++requestId.current
    try {
      const result = await invoke<DuplicateResult>('duplicate_video', { path: file.path })
      const duplicate = result.listing.entries.find(entry => entry.path === result.duplicatedPath)
      if (currentRequest === requestId.current) {
        pendingFocus.current = { path: result.duplicatedPath }
        setListing(result.listing)
        if (duplicate) void selectFile(duplicate)
      }
      setFileAction({ message: `Created “${duplicate?.name ?? displayPath(result.duplicatedPath)}”.`, error: false })
    } catch (cause) {
      setFileAction({ message: reportError('Duplicating video', cause), error: true })
    } finally { setDuplicatingFile(null) }
  }

  function openRename(file: FileEntry) {
    setContextMenu(null)
    setRenameTarget(file)
    setRenameStem(editableName(file))
    setRenameError(null)
  }

  function closeRename() {
    if (renaming) return
    const path = renameTarget?.path
    setRenameTarget(null)
    window.requestAnimationFrame(() => {
      Array.from(fileListRef.current?.querySelectorAll<HTMLButtonElement>('[data-list-row]') ?? [])
        .find(row => row.dataset.path === path)?.focus()
    })
  }

  async function renameFile() {
    const file = renameTarget
    if (!file || renaming) return
    setRenaming(true)
    setRenameError(null)
    const currentRequest = ++requestId.current
    if (!file.isDirectory) flushSync(() => { setSelected(null); setProbe(null); setError(null); setLoading(false) })
    try {
      const result = await invoke<RenameResult>(file.isDirectory ? 'rename_folder' : 'rename_video', file.isDirectory
        ? { path: file.path, newName: renameStem }
        : { path: file.path, newStem: renameStem })
      if (currentRequest !== requestId.current) return
      const renamed = result.listing.entries.find(entry => entry.path === result.renamedPath)
      pendingFocus.current = { path: result.renamedPath }
      setListing(result.listing)
      setRenameTarget(null)
      setFileAction({ message: `Renamed “${file.name}” to “${renamed?.name ?? renameStem}”.`, error: false })
      if (renamed && !renamed.isDirectory) void selectFile(renamed)
    } catch (cause) {
      setRenameError(reportError(file.isDirectory ? 'Renaming folder' : 'Renaming video', cause))
      if (!file.isDirectory) void selectFile(file)
    } finally { setRenaming(false) }
  }

  async function moveEntryToTrash(file: FileEntry) {
    if (trashRequestPending.current || deleting || duplicatingFile || renaming || renameTarget || folderBusy) return
    trashRequestPending.current = true
    setContextMenu(null)
    const paths = file.isDirectory ? [] : selectedPaths.includes(file.path) ? selectedPaths : [file.path]
    const count = paths.length
    try {
      const approved = await ask(file.isDirectory
        ? `Move folder “${file.name}” and everything inside it to Trash? You can restore it from your system's Trash or Recycle Bin.`
        : count === 1
          ? `Move “${file.name}” to Trash? You can restore it from your system's Trash or Recycle Bin.`
          : `Move ${count} selected videos to Trash? You can restore them from your system's Trash or Recycle Bin.`,
      { title: file.isDirectory ? 'Move folder to Trash' : count === 1 ? 'Move video to Trash' : `Move ${count} videos to Trash`, kind: 'warning', okLabel: 'Move to Trash', cancelLabel: 'Cancel' })
      if (!approved) return
      setDeleting(true)
      setFileAction(null)
      const currentRequest = ++requestId.current
      flushSync(() => { setSelected(null); setProbe(null); setError(null); setLoading(false) })
      const result: TrashBatchResult = file.isDirectory
        ? { listing: await invoke<DirectoryListing>('move_folder_to_trash', { path: file.path }), movedCount: 1, error: null }
        : await invoke<TrashBatchResult>('move_videos_to_trash', { paths })
      if (currentRequest !== requestId.current) return
      const next = result.listing
      const oldIndex = mediaEntries.findIndex(entry => entry.path === file.path)
      const nextEntries = sortMedia(next.entries, mediaSort, sortDirection)
      const remaining = result.error ? nextEntries.filter(entry => paths.includes(entry.path)) : []
      const nearby = remaining[0] ?? nextEntries[Math.min(Math.max(oldIndex, 0), nextEntries.length - 1)]
      pendingFocus.current = nearby ? { path: nearby.path } : null
      setListing(next)
      setSelectedPaths(remaining.map(entry => entry.path))
      selectionAnchor.current = null
      const message = result.error
        ? `Moved ${result.movedCount} of ${count} ${count === 1 ? 'video' : 'videos'} to Trash. ${result.error}`
        : file.isDirectory ? `Moved “${file.name}” to Trash.`
          : count === 1 ? `Moved “${file.name}” to Trash.`
            : `Moved ${count} videos to Trash.`
      setFileAction({ message, error: !!result.error })
      if (nearby && !nearby.isDirectory) void selectFile(nearby, 0, remaining.length > 0)
    } catch (cause) {
      setFileAction({ message: reportError(file.isDirectory ? 'Moving folder to Trash' : 'Moving video to Trash', cause), error: true })
      if (listing) {
        try {
          const refreshed = await invoke<DirectoryListing>('list_directory', { path: listing.path })
          setListing(refreshed)
          setSelectedPaths(paths.filter(path => refreshed.entries.some(entry => entry.path === path)))
        } catch (refreshCause) { reportError('Refreshing folder after Trash error', refreshCause) }
      }
    } finally { trashRequestPending.current = false; setDeleting(false) }
  }

  function navigateFiles(event: KeyboardEvent<HTMLDivElement>) {
    const focused = event.target instanceof HTMLElement ? event.target.closest<HTMLButtonElement>('[data-list-row]') : null
    if (!focused || !event.currentTarget.contains(focused)) return
    if (event.key === 'Delete' || (event.key === 'Backspace' && /Mac/i.test(navigator.platform))) {
      event.preventDefault()
      const entry = mediaEntries.find(item => !item.isDirectory && selectedPaths.includes(item.path))
      if (entry && !event.repeat) void moveEntryToTrash(entry)
      return
    }
    if (event.key === 'Enter') {
      const entry = mediaEntries[Number(focused.dataset.entryIndex)]
      if (entry && !entry.isDirectory) {
        // Prevent the button's default click from clearing the playback request.
        event.preventDefault()
        if (!event.repeat) playMediaEntry(entry)
      }
      return
    }
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault()
      const entry = mediaEntries[Number(focused.dataset.entryIndex)]
      if (entry) {
        const bounds = focused.getBoundingClientRect()
        openFileMenu(entry, bounds.left + 24, bounds.bottom - 6)
      }
      return
    }
    if (!['ArrowUp', 'ArrowDown', ...(mediaView === 'grid' ? ['ArrowLeft', 'ArrowRight'] : [])].includes(event.key)) return
    const rows = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[data-list-row]'))
    const currentIndex = rows.indexOf(focused)
    if (currentIndex < 0) return
    event.preventDefault()
    const columns = mediaView === 'grid' ? getComputedStyle(event.currentTarget).gridTemplateColumns.split(' ').length : 1
    const nextIndex = nextMediaIndex(currentIndex, rows.length, columns, event.key)
    if (nextIndex === currentIndex) return
    rows[nextIndex].focus()
    const entryIndex = Number(rows[nextIndex].dataset.entryIndex)
    const next = mediaEntries[entryIndex]
    if (next && (!next.isDirectory || event.ctrlKey || event.metaKey || event.shiftKey)) {
      selectMediaEntry(next, entryIndex, event)
    } else {
      requestId.current++
      setSelected(null); setSelectedPaths([]); selectionAnchor.current = null; setProbe(null); setError(null); setLoading(false)
    }
  }

  async function copyMetadata() {
    if (!probe) return
    try {
      await navigator.clipboard.writeText(JSON.stringify(probe, null, 2))
      setCopyDone(true)
      window.setTimeout(() => setCopyDone(false), 1800)
    } catch (cause) { setError(`Could not copy metadata: ${reportError('Copying metadata', cause)}`) }
  }

  async function finishEditing(result: EditResult | null, replace: boolean) {
    const file = editingFile
    setEditingFile(null)
    if (!result || !listing || !file) return
    await browse(listing.path, { path: file.path })
    const metadataNote = result.metadataWarnings.length > 0
      ? ` Track metadata changed: ${result.metadataWarnings.join('; ')}.`
      : ''
    setFileAction({
      message: (result.backupPath
        ? `Saved to ${displayPath(result.outputPath)}. The original backup remains at ${displayPath(result.backupPath)}.`
        : replace ? `Saved “${file.name}”. The previous version is in Trash.` : `Saved edited video to ${displayPath(result.outputPath)}.`) + metadataNote,
      error: false,
    })
  }

  async function savedEditing(result: EditResult) {
    const file = editingFile
    if (!listing || !file) return
    await browse(listing.path, { path: file.path })
    const metadataNote = result.metadataWarnings.length > 0
      ? ` Track metadata changed: ${result.metadataWarnings.join('; ')}.`
      : ''
    setFileAction({ message: `Saved edited video to ${displayPath(result.outputPath)}.${metadataNote}`, error: false })
  }

  const video = probe?.streams?.find(stream => stream.codec_type === 'video')
  const audio = probe?.streams?.find(stream => stream.codec_type === 'audio')
  const pathParts = listing?.path.split(/[\\/]/).filter(Boolean) ?? []
  const directoryCount = listing?.entries.filter(entry => entry.isDirectory).length ?? 0
  const videoCount = listing?.entries.length ? listing.entries.length - directoryCount : 0

  if (editingFile) return <Editor file={editingFile} onExit={(result, replace) => { void finishEditing(result, replace) }} onSaved={result => { void savedEditing(result) }} />

  return <div className="app-shell">
    <header className="topbar">
      <div className="brand"><div className="brand-mark"><Icon name="film" size={21} /></div><span>Framewise</span><span className="brand-beta">BETA</span></div>
      <span className="topbar-note">Local video inspector</span>
      <button className="choose-button" onClick={chooseFolder}><Icon name="folder" size={17} /> Choose folder</button>
    </header>

    <div className={`workspace ${workspaceCollapsed ? 'left-collapsed' : ''} ${inspectorCollapsed ? 'right-collapsed' : ''} ${resizing ? 'resizing' : ''}`} style={{ '--left-width': `${effectiveWorkspaceWidth}px`, '--right-width': `${effectiveInspectorWidth}px` } as CSSProperties}>
      <aside className={`sidebar ${workspaceCollapsed ? 'collapsed' : ''}`} aria-label="Workspace">
        <div className="sidebar-heading">
          {!workspaceCollapsed && <span>WORKSPACE</span>}
          <button className="panel-toggle workspace-toggle" onClick={toggleWorkspace} aria-label={workspaceCollapsed ? 'Expand workspace' : 'Collapse workspace'} aria-expanded={!workspaceCollapsed} title={workspaceCollapsed ? 'Expand workspace' : 'Collapse workspace'}><Icon name="chevron" size={17} /></button>
        </div>
        {workspaceCollapsed
          ? <button className={`rail-icon media-rail ${view === 'media' ? 'active' : ''}`} onClick={() => setView('media')} title="Your media" aria-label="Your media"><Icon name="film" size={20} /></button>
          : <button className={`sidebar-media ${view === 'media' ? 'active' : ''}`} onClick={() => setView('media')}><Icon name="film" size={18} /> Your media</button>}
        {workspaceCollapsed ? root && <button className="rail-icon" onClick={() => browse(root, {})} title="Go to selected folder" aria-label="Go to selected folder"><Icon name="folder" size={20} /></button> : root ? <>
          <button className="root-item" onClick={() => browse(root, {})} title={displayPath(root)}><Icon name="folder" size={19} /><span>{root.split(/[\\/]/).filter(Boolean).at(-1) || displayPath(root)}</span></button>
          <div className="sidebar-section-label">CURRENT FOLDER</div>
          <div className="sidebar-current" title={listing ? displayPath(listing.path) : undefined}>{listing && displayPath(listing.path)}</div>
        </> : <div className="sidebar-hint">Choose a folder to see your videos here.</div>}
        {workspaceCollapsed
          ? <button className={`rail-icon settings-rail ${view === 'settings' ? 'active' : ''}`} onClick={() => setView('settings')} title="Settings" aria-label="Settings"><Icon name="settings" size={20} /></button>
          : <button className={`sidebar-settings ${view === 'settings' ? 'active' : ''}`} onClick={() => setView('settings')}><Icon name="settings" size={18} /> Settings</button>}
        {!workspaceCollapsed && <div className="sidebar-bottom"><span className="status-dot" /> Files stay on your device</div>}
      </aside>

      <main className="main-panel">
        {view === 'settings' ? <>
          <div className="content-heading">
            <div className="eyebrow">PREFERENCES</div>
            <h1>Settings</h1>
            <p>Choose how Framewise looks and starts on this computer.</p>
          </div>
          <section className="settings-card" aria-labelledby="appearance-heading">
            <div className="settings-card-heading"><div className="settings-card-icon"><Icon name="sun" size={22} /></div><div><h2 id="appearance-heading">Appearance</h2><p>Choose the color theme for Framewise.</p></div></div>
            <div className="theme-options" role="radiogroup" aria-labelledby="appearance-heading">
              <label className={`theme-option ${theme === 'light' ? 'selected' : ''}`}>
                <input type="radio" name="theme" value="light" checked={theme === 'light'} onChange={() => chooseTheme('light')} />
                <Icon name="sun" size={21} /><span>Light</span>
              </label>
              <label className={`theme-option ${theme === 'dark' ? 'selected' : ''}`}>
                <input type="radio" name="theme" value="dark" checked={theme === 'dark'} onChange={() => chooseTheme('dark')} />
                <Icon name="moon" size={21} /><span>Dark</span>
              </label>
            </div>
          </section>
          <section className="settings-card" aria-labelledby="startup-folder-heading">
            <div className="settings-card-heading"><div className="settings-card-icon"><Icon name="folder" size={22} /></div><div><h2 id="startup-folder-heading">Startup folder</h2><p>Open this folder automatically when Framewise starts.</p></div></div>
            <div className="setting-label">SELECTED FOLDER</div>
            <div className={`setting-value ${defaultFolder ? '' : 'empty'}`} title={defaultFolder ? displayPath(defaultFolder) : undefined}>{defaultFolder ? displayPath(defaultFolder) : 'No startup folder selected'}</div>
            {settingsError && <div className="settings-error" role="alert"><Icon name="info" size={18} /><span>{settingsError}</span></div>}
            {startupLoading && <div className="settings-loading">Opening startup folder…</div>}
            <div className="settings-actions">
              <button className="settings-primary" onClick={chooseDefaultFolder}><Icon name="folder" size={17} /> {defaultFolder ? 'Change folder' : 'Choose folder'}</button>
              {root && root !== defaultFolder && <button className="settings-secondary" onClick={useCurrentAsDefault}>Use current folder</button>}
              {defaultFolder && <button className="settings-clear" onClick={clearDefaultFolder}>Clear</button>}
            </div>
            <p className="settings-footnote">The folder path is saved locally. You can still browse other folders at any time.</p>
          </section>
          <section className="settings-card" aria-labelledby="preview-cache-heading">
            <div className="settings-card-heading"><div className="settings-card-icon"><Icon name="film" size={22} /></div><div><h2 id="preview-cache-heading">Preview images</h2><p>Framewise creates a preview image when you select a video for the first time, then reuses it on later selections. If the video changes, a new image is created.</p></div></div>
            <div className="setting-label">CACHE FOLDER · READ ONLY</div>
            <div className={`setting-value ${previewCacheDirectory ? '' : 'empty'}`} title={previewCacheDirectory ? displayPath(previewCacheDirectory) : undefined}>{previewCacheDirectory ? displayPath(previewCacheDirectory) : previewCacheError ? 'Cache folder unavailable' : isTauri() ? 'Locating cache folder…' : 'Available in the desktop app'}</div>
            <div className="setting-label">PREVIEW INDEX · READ ONLY</div>
            <div className={`setting-value ${previewIndexLocation ? '' : 'empty'}`} title={previewIndexLocation ? displayPath(previewIndexLocation) : undefined}>{previewIndexLocation ? displayPath(previewIndexLocation) : previewIndexError ? 'Preview index unavailable' : isTauri() ? 'Locating preview index…' : 'Available in the desktop app'}</div>
            {previewCacheError && <div className="settings-error" role="alert"><Icon name="info" size={18} /><span>{previewCacheError}</span></div>}
            {previewIndexError && <div className="settings-error" role="alert"><Icon name="info" size={18} /><span>{previewIndexError}</span></div>}
            <p className="settings-footnote">Framewise keeps preview images in <code>.framewise/previews</code> and their video associations in <code>.framewise/framewise.db</code>. Images can be regenerated if deleted.</p>
          </section>
        </> : <>
        <div className="content-heading">
          <div className="eyebrow">YOUR MEDIA</div>
          <h1>{listing ? pathParts.at(-1) || 'Videos' : 'Explore your videos'}</h1>
          <p>{listing ? 'Browse folders and select a video to view its metadata.' : 'Select a folder to get started. Your files never leave this computer.'}</p>
        </div>

        {!listing ? <div className="welcome-card">
          <div className="welcome-icon"><Icon name="folder" size={34} /></div>
          <h2>Start with a folder</h2>
          <p>Find your video files and see the details behind every clip.</p>
          <button className="primary-button" onClick={chooseFolder}>Choose a folder <Icon name="chevron" size={18} /></button>
          <div className="supported">MP4 · MOV · MKV · WebM · AVI and more</div>
        </div> : <>
          <div className="breadcrumb"><button onClick={() => root && browse(root, {})}>{root?.split(/[\\/]/).filter(Boolean).at(-1) || 'Root'}</button>{listing.path !== root && <><Icon name="chevron" size={14} /><span>{pathParts.at(-1)}</span></>}</div>
          <div className="browser-toolbar"><span>{directoryCount} {directoryCount === 1 ? 'folder' : 'folders'} <span className="dot-separator">·</span> {videoCount} {videoCount === 1 ? 'video' : 'videos'}{selectedPaths.length > 1 && <> <span className="dot-separator">·</span> {selectedPaths.length} selected</>}</span><div className="browser-toolbar-actions"><button className="browser-new-folder" onClick={() => setCreatingFolder(true)} disabled={folderBusy || creatingFolder}>+ New folder</button><button title="Refresh folder" aria-label="Refresh folder" onClick={() => browse(listing.path, selected ? { path: selected.path } : {})}><Icon name="refresh" size={17} /></button></div></div>
          {creatingFolder && <form className="browser-create-folder" onSubmit={event => { event.preventDefault(); void createMediaFolder() }}><input autoFocus aria-label="New folder name" placeholder="New folder name" value={newFolderName} onChange={event => setNewFolderName(event.target.value)} disabled={folderBusy} /><button type="submit" disabled={folderBusy || !newFolderName.trim()}>{folderBusy ? 'Creating…' : 'Create'}</button><button type="button" onClick={() => { setCreatingFolder(false); setNewFolderName('') }} disabled={folderBusy}>Cancel</button></form>}
          {duplicatingFile && <div className="file-action progress" role="status">Duplicating “{duplicatingFile}”…</div>}
          {deleting && <div className="file-action progress" role="status">Moving to Trash…</div>}
          {fileAction && <div className={`file-action ${fileAction.error ? 'error' : ''}`} role={fileAction.error ? 'alert' : 'status'}>{fileAction.message}</div>}
          <div className="media-view-switch" role="group" aria-label="File view">
            <label className="media-sort">Sort by <select value={mediaSort} onChange={event => { const value = event.target.value as MediaSort; setMediaSort(value); setPreference('framewise.mediaSort', value) }}><option value="name">Filename</option><option value="modified">Date modified</option></select></label>
            <label className="media-sort"><span className="sort-order-label">Order</span><select aria-label="Sort order" value={sortDirection} onChange={event => { const value = event.target.value as SortDirection; setSortDirection(value); setPreference('framewise.sortDirection', value) }}><option value="asc">Ascending</option><option value="desc">Descending</option></select></label>
            {(['list', 'grid'] as const).map(mode => <button key={mode} aria-pressed={mediaView === mode} onClick={() => { setMediaView(mode); setPreference('framewise.mediaView', mode) }}>{mode === 'list' ? 'List' : 'Grid'}</button>)}
            {mediaView === 'grid' && <label className="media-sort">Preview size <select value={gridSize} onChange={event => { const value = event.target.value as typeof gridSize; setGridSize(value); setPreference('framewise.gridSize', value) }}><option value="small">Small</option><option value="medium">Medium</option><option value="large">Large</option></select></label>}
          </div>
          <div ref={fileListRef} className={`file-list ${mediaView === 'grid' ? `media-grid grid-size-${gridSize}` : ''}`} role="group" aria-label="Files and folders" onKeyDown={navigateFiles}>
            {listing.parent && <button className="file-row back-row" data-list-row="true" data-entry-index="-1" data-path={listing.parent} onClick={() => browse(listing.parent!, { path: listing.path })}><span className="file-icon"><Icon name="arrow" size={18} /></span><span className="file-name">Go back</span></button>}
            {mediaEntries.map((entry, index) => <button key={entry.path} data-list-row="true" data-entry-index={index} data-path={entry.path} aria-pressed={selectedPaths.includes(entry.path)} className={`file-row ${selectedPaths.includes(entry.path) ? 'selected' : ''}`} onClick={event => selectMediaEntry(entry, index, event)} onDoubleClick={() => playMediaEntry(entry)} onContextMenu={event => fileContextMenu(event, entry)}>
              {mediaView === 'grid' && !entry.isDirectory
                ? <MediaThumbnail path={entry.path} revision={listing} placeholder={<Icon name="film" size={32} />} />
                : <span className={`file-icon ${entry.isDirectory ? 'folder-icon' : 'video-icon'}`}><Icon name={entry.isDirectory ? 'folder' : 'film'} size={mediaView === 'grid' ? 32 : 19} /></span>}
              <span className="file-name" title={entry.name}>{entry.name}</span>
              <span className="file-kind">{entry.isDirectory ? 'Folder' : fileSize(entry.size)}</span>
              <Icon name="chevron" size={16} />
            </button>)}
            {listing.entries.length === 0 && <div className="empty-list">No folders or supported video files here.</div>}
            {(listing.parent || listing.entries.length > 0) && <div className="file-list-tip">Tip: Ctrl-click (⌘-click on Mac) or Shift-click to select several videos or folders. Right-click a selection to move it. A normal click opens a folder.</div>}
          </div>
        </>}
        </>}
      </main>

      <section className={`details-panel ${inspectorCollapsed ? 'collapsed' : ''}`} aria-label="Video metadata">
        <div className="details-top">
          {!inspectorCollapsed && <div><div className="eyebrow">INSPECTOR</div><h2>File details</h2></div>}
          <div className="details-actions">{!inspectorCollapsed && probe && <button className="icon-button" onClick={copyMetadata} title="Copy raw metadata" aria-label="Copy raw metadata"><Icon name={copyDone ? 'check' : 'copy'} size={17} /></button>}
          <button className="panel-toggle inspector-toggle" onClick={toggleInspector} aria-label={inspectorCollapsed ? 'Expand inspector' : 'Collapse inspector'} aria-expanded={!inspectorCollapsed} title={inspectorCollapsed ? 'Expand inspector' : 'Collapse inspector'}><Icon name="chevron" size={17} /></button></div>
        </div>
        {!inspectorCollapsed && (selected ? <div key={selected.path} className="details-body">
          <VideoPreview file={selected} playRequested={playbackPath === selected.path} onPlayRequestHandled={() => setPlaybackPath(null)} />
          <div className="selected-file"><span className="selected-file-icon"><Icon name="film" size={27} /></span><div><strong title={selected.name}>{selected.name}</strong><span>{fileSize(selected.size)}</span></div></div>
          {probe && video && <button className="inspector-edit" onClick={() => editFile(selected)}>Edit video</button>}
          {loading && <div className="notice">Reading video metadata…</div>}
          {error && <div className="notice error" role="alert"><Icon name="info" size={18} /><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss error"><Icon name="close" size={15} /></button></div>}
          {probe && <>
            <GenerationPrompt probe={probe} />
            <div className="section-title">OVERVIEW</div>
            <dl className="property-list"><Property label="Duration" value={duration(probe.format?.duration)} /><Property label="File size" value={fileSize(probe.format?.size ?? selected.size)} /><Property label="Container" value={probe.format?.format_long_name ?? probe.format?.format_name} /><Property label="Bitrate" value={bitrate(probe.format?.bit_rate)} /></dl>
            <div className="section-title">VIDEO STREAM</div>
            {video ? <dl className="property-list"><Property label="Codec" value={video.codec_name?.toUpperCase()} /><Property label="Resolution" value={video.width && video.height ? `${video.width} × ${video.height}` : undefined} /><Property label="Frame rate" value={frameRate(video.avg_frame_rate ?? video.r_frame_rate)} /><Property label="Pixel format" value={video.pix_fmt} /><Property label="Color space" value={video.color_space} /></dl> : <p className="no-data">No video stream found.</p>}
            <div className="section-title">AUDIO STREAM</div>
            {audio ? <dl className="property-list"><Property label="Codec" value={audio.codec_name?.toUpperCase()} /><Property label="Channels" value={audio.channel_layout ?? audio.channels} /><Property label="Sample rate" value={audio.sample_rate ? `${Number(audio.sample_rate).toLocaleString()} Hz` : undefined} /></dl> : <p className="no-data">No audio stream found.</p>}
            <div className="section-title">EMBEDDED TAGS</div>
            {Object.keys(probe.format?.tags ?? {}).length ? <dl className="property-list">{Object.entries(probe.format?.tags ?? {}).map(([key, value]) => <Property key={key} label={key} value={value} />)}</dl> : <p className="no-data">No embedded tags found.</p>}
            <details className="raw-details"><summary>View raw metadata</summary><pre>{JSON.stringify(probe, null, 2)}</pre></details>
          </>}
        </div> : <div className="details-empty"><div className="details-empty-icon"><Icon name="info" size={27} /></div><h3>Nothing selected</h3><p>Choose a video from the browser to see its metadata here.</p>{error && <div className="notice error" role="alert">{error}</div>}</div>)}
      </section>
      {!workspaceCollapsed && <div className="column-resizer left-resizer" role="separator" tabIndex={0} aria-label="Resize workspace column" aria-orientation="vertical" aria-valuemin={MIN_WORKSPACE_WIDTH} aria-valuemax={widthLimits('left').max} aria-valuenow={effectiveWorkspaceWidth} onPointerDown={event => startResize('left', event)} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize} onKeyDown={event => resizeWithKeyboard('left', event)} />}
      {!inspectorCollapsed && <div className="column-resizer right-resizer" role="separator" tabIndex={0} aria-label="Resize inspector column" aria-orientation="vertical" aria-valuemin={MIN_INSPECTOR_WIDTH} aria-valuemax={widthLimits('right').max} aria-valuenow={effectiveInspectorWidth} onPointerDown={event => startResize('right', event)} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize} onKeyDown={event => resizeWithKeyboard('right', event)} />}
    </div>
    {contextMenu && <div ref={contextMenuRef} className="file-context-menu" role="menu" aria-label={`Actions for ${contextMenu.file.name}`} style={{ left: contextMenu.x, top: contextMenu.y }}>
      <button className="reveal-menu-item" role="menuitem" onClick={() => void showInFileManager(contextMenu.file)}>{/Win/i.test(navigator.platform) ? 'Show in Explorer' : /Mac/i.test(navigator.platform) ? 'Show in Finder' : 'Show in File Manager'}</button>
      {contextMenu.file.isDirectory ? <>
        <button className="rename-menu-item" role="menuitem" onClick={() => openRename(contextMenu.file)}>Rename folder</button>
        <button className="move-menu-item" role="menuitem" onClick={() => void openMoveWindow(contextMenu.file)}>Move to…{selectedPaths.includes(contextMenu.file.path) && selectedPaths.length > 1 ? ` (${selectedPaths.length})` : ''}</button>
        <button role="menuitem" onClick={() => void moveEntryToTrash(contextMenu.file)}>Move folder to Trash</button>
      </> : <>
        <button className="edit-menu-item" role="menuitem" onClick={() => editFile(contextMenu.file)}>Edit video</button>
        <button className="rename-menu-item" role="menuitem" onClick={() => openRename(contextMenu.file)}>Rename</button>
        <button className="move-menu-item" role="menuitem" onClick={() => void openMoveWindow(contextMenu.file)}>Move to…{selectedPaths.includes(contextMenu.file.path) && selectedPaths.length > 1 ? ` (${selectedPaths.length})` : ''}</button>
        <button className="duplicate-menu-item" role="menuitem" onClick={() => void duplicateFile(contextMenu.file)}>Duplicate</button>
        <button role="menuitem" onClick={() => void moveEntryToTrash(contextMenu.file)}>{selectedPaths.includes(contextMenu.file.path) && selectedPaths.length > 1 ? `Move ${selectedPaths.length} videos to Trash` : 'Move to Trash'}</button>
      </>}
    </div>}
    {renameTarget && <div className="rename-backdrop">
      <form className="rename-dialog" role="dialog" aria-modal="true" aria-labelledby="rename-title" onSubmit={event => { event.preventDefault(); void renameFile() }} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); closeRename() } }}>
        <h2 id="rename-title">Rename {renameTarget.isDirectory ? 'folder' : 'video'}</h2>
        <label htmlFor="rename-name">{renameTarget.isDirectory ? 'Folder name' : 'File name'}</label>
        <div className="rename-name-field"><input id="rename-name" autoFocus value={renameStem} onFocus={event => event.currentTarget.select()} onChange={event => setRenameStem(event.target.value)} disabled={renaming} />{!renameTarget.isDirectory && <span>{fixedExtension(renameTarget)}</span>}</div>
        {renameError && <p className="rename-error" role="alert">{renameError}</p>}
        <div className="rename-actions"><button type="button" onClick={closeRename} disabled={renaming}>Cancel</button><button type="submit" disabled={renaming || !renameStem.trim() || renameStem === editableName(renameTarget)}>{renaming ? 'Renaming…' : 'Rename'}</button></div>
      </form>
    </div>}
  </div>
}

export default App
