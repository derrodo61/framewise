import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import type { CSSProperties, KeyboardEvent, MouseEvent, PointerEvent } from 'react'
import { convertFileSrc, invoke, isTauri } from '@tauri-apps/api/core'
import { ask, open } from '@tauri-apps/plugin-dialog'
import { error as logError, info as logInfo } from '@tauri-apps/plugin-log'
import { listen } from '@tauri-apps/api/event'
import { WebviewWindow } from '@tauri-apps/api/webviewWindow'
import Editor from './Editor'
import GenerationPrompt from './GenerationPrompt'
import TagSettings from './TagSettings'
import AppVersion from './AppVersion'
import TagFilter from './TagFilter'
import { useMediaBrowser, useMediaSelection } from './useMediaBrowser'
import { folderResults } from './mediaResults'
import { prepareTrash, executeTrash, trashMessage, folderTrashResult, folderTrashBatch } from './trashWorkflow'
import type { FileEntry, DirectoryListing, TrashBatchResult } from './mediaModel'
import { parentPath, sameParent } from './mediaFilter'
import { VideoTags, TagBatchDialog } from './VideoTags'
import type { NamedTagVideo } from './VideoTags'
import MediaThumbnail from './MediaThumbnail'
import ImagePreview from './ImagePreview'
import { isImage } from './mediaTypes'
import DateFilter from './DateFilter'
import type { DateFilterState } from './mediaDates'
import { generateThumbnail } from './thumbnailQueue'
import { nextMediaIndex } from './mediaNavigation'
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

function Icon({ name, size = 20 }: { name: 'folder' | 'film' | 'image' | 'chevron' | 'arrow' | 'info' | 'refresh' | 'copy' | 'check' | 'close' | 'settings' | 'sun' | 'moon' | 'play'; size?: number }) {
  const paths: Record<typeof name, React.ReactNode> = {
    folder: <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />,
    film: <><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M7 4v16M17 4v16M3 9h4m-4 6h4m10-6h4m-4 6h4" /></>,
    image: <><rect x="3" y="3" width="18" height="18" rx="2" /><circle cx="8" cy="8" r="1.5" /><path d="m3 17 6-6 4 4 3-3 5 5" /></>,
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

function EmbeddedTags({ tags }: { tags: Record<string, string> }) {
  const entries = Object.entries(tags)
  if (!entries.length) return <p className="no-data">No embedded tags found.</p>
  const compact = entries.filter(([, value]) => value.length <= 500)
  return <>
    {compact.length > 0 && <dl className="property-list">{compact.map(([key, value]) => <Property key={key} label={key} value={value} />)}</dl>}
    {entries.filter(([, value]) => value.length > 500).map(([key, value]) => <details key={key} className="raw-details"><summary>{key}</summary><pre>{value}</pre></details>)}
  </>
}

function VideoPreview({ file, playbackAction, onPlayRequestHandled }: { file: FileEntry; playbackAction: 'play' | 'toggle' | null; onPlayRequestHandled: () => void }) {
  const videoRef = useRef<HTMLVideoElement | null>(null)
  const lookupStartedAt = useRef(0)
  const [preview, setPreview] = useState<Preview | null>(null)
  const [previewError, setPreviewError] = useState<string | null>(null)
  const [thumbnailPending, setThumbnailPending] = useState(false)
  const [playerActive, setPlayerActive] = useState(false)
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
    const video = videoRef.current
    if (!video) return
    setPlaybackError(false)
    setPlayerActive(true)
    void video.play().catch(cause => {
      // Pausing while playback is still starting can abort the play promise.
      if (cause instanceof DOMException && cause.name === 'AbortError') return
      if (videoRef.current !== video) return
      setPlayerActive(false); setPlaybackError(true)
    })
  }

  const playFromList = useEffectEvent(() => {
    const video = videoRef.current
    if (playbackAction === 'toggle' && video && !video.paused && !video.ended) video.pause()
    else startPlayback()
    onPlayRequestHandled()
  })
  useEffect(() => {
    if (playbackAction && videoUrl) playFromList()
  }, [playbackAction, videoUrl])

  return <section className="preview-section" aria-label="Video preview">
    <div className="section-title preview-title">PREVIEW</div>
    <div className={`preview-frame ${playerActive ? 'is-playing' : ''}`}>
      {videoUrl && <video ref={videoRef} src={videoUrl} poster={thumbnailUrl ?? undefined} preload="none" controls={playerActive} playsInline onError={() => { setPlayerActive(false); setPlaybackError(true) }} aria-label={`Video ${file.name}`} />}
      {!playerActive && (thumbnailUrl ? <img src={thumbnailUrl} alt={`Preview frame from ${file.name}`} onLoad={() => {
        const elapsed = Math.round(performance.now() - lookupStartedAt.current)
        if (elapsed > 250) void logInfo(`Preview image displayed after ${elapsed} ms: ${file.name}`)
      }} /> : <div className="preview-placeholder"><Icon name="film" size={36} /></div>)}
      {preview && !playerActive && <button className="preview-play" onClick={startPlayback} aria-label={`Play ${file.name}`}><Icon name="play" size={24} /></button>}
      {(!preview || (thumbnailPending && !playerActive)) && !previewError && <span className="preview-loading">Preparing preview…</span>}
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
  const [selected, setSelected] = useState<FileEntry | null>(null)
  const [playbackRequest, setPlaybackRequest] = useState<{ path: string; action: 'play' | 'toggle' } | null>(null)
  const [probe, setProbe] = useState<Probe | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [copyDone, setCopyDone] = useState(false)
  const [editingFile, setEditingFile] = useState<FileEntry | null>(null)
  const [contextMenu, setContextMenu] = useState<{ file: FileEntry; x: number; y: number } | null>(null)
  const [tagBatch, setTagBatch] = useState<NamedTagVideo[] | null>(null)
  const [tagRevision, setTagRevision] = useState(0)
  const tagReturnPath = useRef<string | null>(null)
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
  const [settingsTab, setSettingsTab] = useState<'settings' | 'tags'>('settings')
  const browser = useMediaBrowser({ root, listing, revision: tagRevision, view, sort: mediaSort, direction: sortDirection })
  const { showVideos, showImages, dateScope, dates, workspace, tagFilter, workspaceSearch, sortedMediaEntries, mediaEntries, resultsReady, scanRunning } = browser
  const selection = useMediaSelection(mediaEntries, resultsReady)
  const { selectedPaths, setSelectedPaths, selectionAnchor } = selection
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
  }, [setSelectedPaths, selectionAnchor])

  const focusPendingFiles = useEffectEvent(() => {
    if (view !== 'media' || !listing || !pendingFocus.current || !resultsReady) return
    const rows = Array.from(fileListRef.current?.querySelectorAll<HTMLButtonElement>('[data-list-row]') ?? [])
    const target = pendingFocus.current.path
      ? rows.find(row => row.dataset.path === pendingFocus.current?.path) ?? rows[0]
      : rows.find(row => row.dataset.entryIndex === '0') ?? rows[0]
    pendingFocus.current = null
    target?.focus()
    const entry = target ? mediaEntries[Number(target.dataset.entryIndex)] : null
    if (entry && !entry.isDirectory && selected?.path !== entry.path) void selectFile(entry)
  })

  const reconcileFilteredSelection = useEffectEvent(() => {
    if (!resultsReady) return
    selection.reconcile()
    if (selected && !mediaEntries.some(entry => entry.path === selected.path)) {
      requestId.current++; setSelected(null); setProbe(null); setError(null); setLoading(false); setPlaybackRequest(null); setContextMenu(null)
    }
  })
  useEffect(() => {
    // Reconcile selection before restoring focus to the newly rendered rows.
    const frame = requestAnimationFrame(() => { reconcileFilteredSelection(); focusPendingFiles() })
    return () => cancelAnimationFrame(frame)
  }, [listing, view, mediaEntries, resultsReady])

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
    : Math.max(MIN_WORKSPACE_WIDTH, Math.min(workspaceWidth, availableForPanels - (view === 'settings' ? 0 : inspectorCollapsed ? COLLAPSED_WIDTH : MIN_INSPECTOR_WIDTH)))
  const effectiveInspectorWidth = inspectorCollapsed
    ? COLLAPSED_WIDTH
    : Math.max(MIN_INSPECTOR_WIDTH, Math.min(inspectorWidth, availableForPanels - effectiveWorkspaceWidth))

  function widthLimits(side: PanelSide) {
    return side === 'left'
      ? { min: MIN_WORKSPACE_WIDTH, max: availableForPanels - (view === 'settings' ? 0 : effectiveInspectorWidth) }
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

  function openSettings() {
    setContextMenu(null)
    setPlaybackRequest(null)
    setView('settings')
  }
  function openMedia() { setTagRevision(value => value + 1); setView('media') }
  function changeTagFilters(ids: number[], matchAll: boolean) {
    requestId.current++; setSelected(null); setSelectedPaths([]); setProbe(null); setError(null); setLoading(false); setPlaybackRequest(null); setContextMenu(null); selectionAnchor.current = null
    tagFilter.change(ids, matchAll)
  }
  function changeSearchScope(workspace: boolean) {
    requestId.current++; setSelected(null); setSelectedPaths([]); setProbe(null); setError(null); setLoading(false); setPlaybackRequest(null); setContextMenu(null); selectionAnchor.current = null; pendingFocus.current = null
    browser.setWorkspace(workspace)
  }
  function changeMediaTypes(videos: boolean, images: boolean) {
    changeTagFilters(tagFilter.ids, tagFilter.matchAll)
    pendingFocus.current = null
    browser.setTypes(videos, images)
  }
  function changeDates(value: DateFilterState) {
    changeTagFilters(tagFilter.ids, tagFilter.matchAll)
    pendingFocus.current = null
    browser.setDates(value)
  }
  function changeSearchPage(page: number) {
    changeTagFilters(tagFilter.ids, tagFilter.matchAll)
    workspaceSearch.setPage(page)
    fileListRef.current?.scrollTo({ top: 0 })
  }

  async function browse(path: string, focusTarget?: { path?: string }) {
    if (workspace) changeSearchScope(false)
    try {
      setPlaybackRequest(null)
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
    if (tagFilter.active) return
    const entries = folderResults(directory.entries, { ...browser.resultFilters, tagActive: false })
    const entry = focusTarget.path
      ? entries.find(item => item.path === focusTarget.path)
      : entries[0]
    if (entry && !entry.isDirectory) void selectFile(entry)
  }

  async function selectFile(file: FileEntry, delayMs = 0, preserveMulti = false) {
    setPlaybackRequest(null)
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
      if (currentRequest === requestId.current) setError(reportError('Inspecting media', cause))
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
    const noun = directories ? (count === 1 ? 'folder' : 'folders') : (count === 1 ? 'file' : 'files')
    const message = `Moved ${count} ${noun} to ${displayPath(destination)}.`
    if (workspace) {
      setTagRevision(value => value + 1)
      setFileAction({ message, error: false })
      return
    }
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
    setContextMenu({ file, x: Math.max(8, Math.min(x, window.innerWidth - 200)), y: Math.max(8, Math.min(y, window.innerHeight - (file.isDirectory ? 180 : 295))) })
  }

  function openVideoTags(file: FileEntry) {
    setContextMenu(null)
    const entries = selectedPaths.includes(file.path) ? mediaEntries.filter(entry => selectedPaths.includes(entry.path)) : [file]
    if (!entries.length || entries.some(entry => entry.isDirectory || !entry.videoId)) {
      setFileAction({ message: 'Refresh the folder to load organization records for every selected file before editing tags.', error: true })
      return
    }
    tagReturnPath.current = file.path
    setTagBatch(entries.map(entry => ({ videoId: entry.videoId!, path: entry.path, name: entry.name })))
  }
  function closeVideoTags() {
    const path = tagReturnPath.current
    setTagBatch(null)
    window.requestAnimationFrame(() => { Array.from(fileListRef.current?.querySelectorAll<HTMLButtonElement>('[data-list-row]') ?? []).find(row => row.dataset.path === path)?.focus() })
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
  function selectAllShownFiles() {
    if (!resultsReady || deleting || renaming || duplicatingFile || renameTarget || trashRequestPending.current) return
    const entry = selection.selectAll(selected)
    if (!entry) return
    setContextMenu(null)
    if (selected?.path !== entry.path) inspectSelection(entry, true)
    window.requestAnimationFrame(() => Array.from(fileListRef.current?.querySelectorAll<HTMLButtonElement>('[data-list-row]') ?? []).find(row => row.dataset.path === entry.path)?.focus())
  }
  function clearFileSelection() {
    setContextMenu(null); setPlaybackRequest(null); selectionAnchor.current = null; pendingFocus.current = null
    inspectSelection(undefined)
  }

  function selectMediaEntry(entry: FileEntry, index: number, modifiers: { shiftKey: boolean; ctrlKey: boolean; metaKey: boolean }) {
    const update = selection.selectEntry(entry, index, modifiers)
    if (update.openFolder) { void browse(update.openFolder); return }
    inspectSelection(update.inspect, true)
  }

  function playMediaEntry(entry: FileEntry, action: 'play' | 'toggle' = 'play') {
    if (entry.isDirectory) return
    // Select explicitly so modifier-clicks cannot leave a different video in the Inspector.
    selectionAnchor.current = entry.path
    if (selected?.path !== entry.path) void selectFile(entry)
    else setSelectedPaths([entry.path])
    if (inspectorCollapsed) {
      setInspectorCollapsed(false)
      setPreference('framewise.inspectorCollapsed', 'false')
    }
    if (!isImage(entry.path)) setPlaybackRequest({ path: entry.path, action })
  }

  async function openMoveWindow(file: FileEntry) {
    setContextMenu(null)
    if (scanRunning) { setFileAction({ message: 'Wait for the workspace scan to finish or cancel it before moving files.', error: true }); return }
    const paths = selectedPaths.includes(file.path) ? selectedPaths : [file.path]
    if (!sameParent(paths)) {
      setFileAction({ message: 'Select files from one folder at a time for Move To. Batch tagging works across folders.', error: true })
      return
    }
    try {
      const existing = await WebviewWindow.getByLabel('move-to')
      if (existing) { await existing.setFocus(); return }
      await invoke('begin_move', { paths })
      const moveWindow = new WebviewWindow('move-to', { url: 'index.html?moveTo=1', title: 'Move to — Framewise', width: 650, height: 680, minWidth: 480, minHeight: 500 })
      await moveWindow.once('tauri://error', event => setFileAction({ message: `Could not open Move To window: ${String(event.payload)}`, error: true }))
    } catch (cause) { setFileAction({ message: reportError('Opening Move To window', cause), error: true }) }
  }

  function editFile(file: FileEntry) {
    if (scanRunning || isImage(file.path)) return
    setContextMenu(null)
    setEditingFile(file)
  }

  function fileContextMenu(event: MouseEvent<HTMLButtonElement>, file: FileEntry) {
    event.preventDefault()
    openFileMenu(file, event.clientX, event.clientY)
  }

  async function duplicateFile(file: FileEntry) {
    if (scanRunning) return
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
      setFileAction({ message: reportError('Duplicating file', cause), error: true })
    } finally { setDuplicatingFile(null) }
  }

  function openRename(file: FileEntry) {
    if (scanRunning) return
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
      setRenameError(reportError(file.isDirectory ? 'Renaming folder' : 'Renaming file', cause))
      if (!file.isDirectory) void selectFile(file)
    } finally { setRenaming(false) }
  }

  async function moveEntryToTrash(file: FileEntry) {
    if (scanRunning) { setFileAction({ message: 'Wait for the workspace scan to finish or cancel it before deleting files.', error: true }); return }
    if (trashRequestPending.current || deleting || duplicatingFile || renaming || renameTarget || folderBusy) return
    trashRequestPending.current = true
    setContextMenu(null)
    let paths: string[] = []
    try {
      const request = prepareTrash({ file, selectedPaths, entries: mediaEntries, ready: resultsReady, root, dates, dateState: dateScope.value, showVideos, showImages })
      paths = request.kind === 'files' ? request.paths : []
      let currentRequest = 0
      const outcome = await executeTrash(request, {
        confirm: request => ask(request.message, { title: request.title, kind: 'warning', okLabel: 'Move to Trash', cancelLabel: 'Cancel' }),
        beforeMove: () => {
          setDeleting(true); setFileAction(null)
          currentRequest = ++requestId.current
          flushSync(() => { setSelected(null); setProbe(null); setError(null); setLoading(false) })
        },
        move: request => request.kind === 'folder'
          ? invoke<DirectoryListing>('move_folder_to_trash', { path: request.paths[0] }).then(folderTrashBatch)
          : invoke<TrashBatchResult>('move_videos_to_trash', { paths: request.paths, filter: request.filter }),
      })
      if (outcome.status === 'cancelled') return
      const result = outcome.result
      if (currentRequest !== requestId.current) return
      if (workspace && !file.isDirectory) {
        const remaining = (result.remainingPaths ?? []).filter(path => mediaEntries.some(entry => entry.path === path))
        pendingFocus.current = null
        setSelectedPaths(remaining); selectionAnchor.current = null
        setTagRevision(value => value + 1)
        setFileAction({ message: trashMessage(request, result), error: !!result.error })
        return
      }
      const next = result.listing
      const nextEntries = folderResults(next.entries, browser.resultFilters)
      const { remaining, nearby } = folderTrashResult(nextEntries, paths, file.path, mediaEntries, result.error)
      pendingFocus.current = nearby ? { path: nearby.path } : null
      setListing(next)
      setSelectedPaths(remaining.map(entry => entry.path))
      selectionAnchor.current = null
      setFileAction({ message: trashMessage(request, result), error: !!result.error })
      if (nearby && !nearby.isDirectory) void selectFile(nearby, 0, remaining.length > 0)
    } catch (cause) {
      setFileAction({ message: reportError(file.isDirectory ? 'Moving folder to Trash' : 'Moving file to Trash', cause), error: true })
      if (listing) {
        try {
          const refreshed = await invoke<DirectoryListing>('list_directory', { path: workspace ? parentPath(file.path) : listing.path })
          setListing(refreshed)
          setSelectedPaths(paths.filter(path => refreshed.entries.some(entry => entry.path === path)))
        } catch (refreshCause) { reportError('Refreshing folder after Trash error', refreshCause) }
      }
    } finally { trashRequestPending.current = false; setDeleting(false) }
  }

  function navigateFiles(event: KeyboardEvent<HTMLDivElement>) {
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'a') {
      event.preventDefault()
      if (!event.repeat) selectAllShownFiles()
      return
    }
    const focused = event.target instanceof HTMLElement ? event.target.closest<HTMLButtonElement>('[data-list-row]') : null
    if (!focused || !event.currentTarget.contains(focused)) return
    if (event.key === 'Delete' || (event.key === 'Backspace' && /Mac/i.test(navigator.platform))) {
      event.preventDefault()
      const entry = mediaEntries.find(item => !item.isDirectory && selectedPaths.includes(item.path))
      if (entry && !event.repeat) void moveEntryToTrash(entry)
      return
    }
    if (event.key === 'Enter' || event.key === ' ') {
      const entry = mediaEntries[Number(focused.dataset.entryIndex)]
      if (entry && !entry.isDirectory) {
        // Prevent Space scrolling or a button click from clearing the playback request.
        event.preventDefault()
        if (event.key === ' ' && isImage(entry.path)) return
        if (!event.repeat) playMediaEntry(entry, 'toggle')
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
    await browse(workspace ? parentPath(file.path) : listing.path, { path: file.path })
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
    await browse(workspace ? parentPath(file.path) : listing.path, { path: file.path })
    const metadataNote = result.metadataWarnings.length > 0
      ? ` Track metadata changed: ${result.metadataWarnings.join('; ')}.`
      : ''
    setFileAction({ message: `Saved edited video to ${displayPath(result.outputPath)}.${metadataNote}`, error: false })
  }

  const video = probe?.streams?.find(stream => stream.codec_type === 'video')
  const selectedImage = selected !== null && isImage(selected.path)
  const audio = probe?.streams?.find(stream => stream.codec_type === 'audio')
  const pathParts = listing?.path.split(/[\\/]/).filter(Boolean) ?? []
  const directoryCount = workspace ? 0 : listing?.entries.filter(entry => entry.isDirectory).length ?? 0
  const totalVideoCount = workspace ? workspaceSearch.totalVideos : sortedMediaEntries.filter(entry => !entry.isDirectory).length
  const videoCount = mediaEntries.filter(entry => !entry.isDirectory).length

  if (editingFile) return <Editor file={editingFile} onExit={(result, replace) => { void finishEditing(result, replace) }} onSaved={result => { void savedEditing(result) }} />

  return <div className="app-shell">
    <header className="topbar">
      <div className="brand"><div className="brand-mark"><Icon name="film" size={21} /></div><span>Framewise</span><span className="brand-beta">BETA</span><AppVersion /></div>
      <span className="topbar-note">Local video inspector</span>
      <button className="choose-button" onClick={chooseFolder}><Icon name="folder" size={17} /> Choose folder</button>
    </header>

    <div className={`workspace ${view === 'settings' ? 'settings-view' : ''} ${workspaceCollapsed ? 'left-collapsed' : ''} ${inspectorCollapsed ? 'right-collapsed' : ''} ${resizing ? 'resizing' : ''}`} style={{ '--left-width': `${effectiveWorkspaceWidth}px`, '--right-width': `${effectiveInspectorWidth}px` } as CSSProperties}>
      <aside className={`sidebar ${workspaceCollapsed ? 'collapsed' : ''}`} aria-label="Workspace">
        <div className="sidebar-heading">
          {!workspaceCollapsed && <span>WORKSPACE</span>}
          <button className="panel-toggle workspace-toggle" onClick={toggleWorkspace} aria-label={workspaceCollapsed ? 'Expand workspace' : 'Collapse workspace'} aria-expanded={!workspaceCollapsed} title={workspaceCollapsed ? 'Expand workspace' : 'Collapse workspace'}><Icon name="chevron" size={17} /></button>
        </div>
        {workspaceCollapsed
          ? <button className={`rail-icon media-rail ${view === 'media' ? 'active' : ''}`} onClick={openMedia} title="Your media" aria-label="Your media"><Icon name="film" size={20} /></button>
          : <button className={`sidebar-media ${view === 'media' ? 'active' : ''}`} onClick={openMedia}><Icon name="film" size={18} /> Your media</button>}
        {workspaceCollapsed ? root && <button className="rail-icon" onClick={() => browse(root, {})} title="Go to selected folder" aria-label="Go to selected folder"><Icon name="folder" size={20} /></button> : root ? <>
          <button className="root-item" onClick={() => browse(root, {})} title={displayPath(root)}><Icon name="folder" size={19} /><span>{root.split(/[\\/]/).filter(Boolean).at(-1) || displayPath(root)}</span></button>
          <div className="sidebar-section-label">CURRENT FOLDER</div>
          <div className="sidebar-current" title={listing ? displayPath(listing.path) : undefined}>{listing && displayPath(listing.path)}</div>
        </> : <div className="sidebar-hint">Choose a folder to see your media here.</div>}
        {workspaceCollapsed
          ? <button className={`rail-icon settings-rail ${view === 'settings' ? 'active' : ''}`} onClick={openSettings} title="Settings" aria-label="Settings"><Icon name="settings" size={20} /></button>
          : <button className={`sidebar-settings ${view === 'settings' ? 'active' : ''}`} onClick={openSettings}><Icon name="settings" size={18} /> Settings</button>}
        {!workspaceCollapsed && <div className="sidebar-bottom"><span className="status-dot" /> Files stay on your device</div>}
      </aside>

      <main className="main-panel">
        {view === 'settings' ? <>
          <div className="content-heading">
            <div className="eyebrow">PREFERENCES</div>
            <h1>Settings</h1>
            <p>Manage your tags and choose how Framewise looks and starts.</p>
          </div>
          <div className="settings-tabs" role="tablist" aria-label="Settings sections" onKeyDown={event => {
            if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
            if (!(event.target instanceof HTMLElement) || event.target.getAttribute('role') !== 'tab') return
            event.preventDefault()
            const next = event.key === 'Home' ? 'settings' : event.key === 'End' ? 'tags' : settingsTab === 'settings' ? 'tags' : 'settings'
            setSettingsTab(next)
            event.currentTarget.closest('main')?.scrollTo({ top: 0 })
            event.currentTarget.querySelector<HTMLButtonElement>(`#${next}-tab`)?.focus()
          }}>
            {(['settings', 'tags'] as const).map(tab => <button key={tab} id={`${tab}-tab`} role="tab" aria-selected={settingsTab === tab} aria-controls={`${tab}-tab-panel`} tabIndex={settingsTab === tab ? 0 : -1} onClick={event => { setSettingsTab(tab); event.currentTarget.closest('main')?.scrollTo({ top: 0 }) }}>{tab === 'settings' ? 'Settings' : 'Tags'}</button>)}
          </div>
          <div id="tags-tab-panel" role="tabpanel" aria-labelledby="tags-tab" hidden={settingsTab !== 'tags'}><TagSettings /></div>
          <div id="settings-tab-panel" role="tabpanel" aria-labelledby="settings-tab" hidden={settingsTab !== 'settings'}>
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
          </div>
        </> : <>
        <div className="content-heading">
          <div className="eyebrow">YOUR MEDIA</div>
          <h1>{listing ? pathParts.at(-1) || 'Media' : 'Explore your media'}</h1>
          <p>{listing ? 'Browse folders and select a media file to view its metadata.' : 'Select a folder to get started. Your files never leave this computer.'}</p>
        </div>

        {!listing ? <div className="welcome-card">
          <div className="welcome-icon"><Icon name="folder" size={34} /></div>
          <h2>Start with a folder</h2>
          <p>Find your media files and see the details behind every clip.</p>
          <button className="primary-button" onClick={chooseFolder}>Choose a folder <Icon name="chevron" size={18} /></button>
          <div className="supported">Videos: MP4 · MOV · MKV · WebM · AVI and more — Images: JPG · PNG · WebP</div>
        </div> : <>
          <div className="breadcrumb"><button onClick={() => root && browse(root, {})}>{root?.split(/[\\/]/).filter(Boolean).at(-1) || 'Root'}</button>{listing.path !== root && <><Icon name="chevron" size={14} /><span>{pathParts.at(-1)}</span></>}</div>
          <TagFilter filter={tagFilter} count={workspace ? workspaceSearch.total : videoCount} total={totalVideoCount} onChange={changeTagFilters} workspace={workspace} onScopeChange={changeSearchScope} ready={resultsReady} />
          <div className="media-type-filters" role="group" aria-label="Media types"><label><input type="checkbox" checked={showVideos} onChange={event => changeMediaTypes(event.target.checked, showImages)} /> Videos</label><label><input type="checkbox" checked={showImages} onChange={event => changeMediaTypes(showVideos, event.target.checked)} /> Images</label></div>
          <DateFilter value={dateScope.value} error={dates.error} onChange={changeDates} />
          {workspace && <div className="workspace-discovery">
            <div className="workspace-scan-actions"><span role="status">{scanRunning ? 'Scanning workspace…' : workspaceSearch.scan?.status === 'complete' ? 'Scan complete' : workspaceSearch.scan?.status === 'cancelled' ? 'Scan cancelled — results may be incomplete' : 'Workspace catalog'} · {workspaceSearch.scan?.folders ?? 0} folders · {workspaceSearch.scan?.videos ?? 0} files discovered</span><button onClick={scanRunning ? workspaceSearch.cancel : workspaceSearch.rescan}>{scanRunning ? 'Cancel scan' : 'Scan workspace'}</button></div>
            {scanRunning && <><progress aria-label="Scanning workspace" /><p title={displayPath(workspaceSearch.scan!.currentFolder)}>{displayPath(workspaceSearch.scan!.currentFolder)}</p><p>File changes and tag assignments are available when scanning finishes.</p></>}
            {workspaceSearch.scan?.message && <p role="status">{workspaceSearch.scan.warnings > 0 && `${workspaceSearch.scan.warnings} folder(s) skipped. `}{workspaceSearch.scan.message}</p>}
            {(workspaceSearch.scanError || workspaceSearch.error) && <p role="alert">{workspaceSearch.scanError || workspaceSearch.error}</p>}
            <p>Results show catalogued media, including previously discovered files. Scan to refresh files added or removed outside Framewise.</p>
          </div>}
          <div className="browser-toolbar"><span>{workspace ? 'Workspace results' : `${directoryCount} ${directoryCount === 1 ? 'folder' : 'folders'}`} <span className="dot-separator">·</span> {!resultsReady ? 'Loading results…' : `${videoCount} ${videoCount === 1 ? 'file' : 'files'}${workspace ? ' on this page' : ''}`}{selectedPaths.length > 1 && <> <span className="dot-separator">·</span> {selectedPaths.length} selected</>}</span><div className="browser-toolbar-actions"><button className="selection-action" title={workspace ? 'Select the files on this result page, excluding folders' : 'Select all files matching the current filters, excluding folders'} onClick={selectAllShownFiles} disabled={!resultsReady || videoCount === 0 || deleting || renaming || !!duplicatingFile || !!renameTarget}>Select all shown</button><button className="selection-action" onClick={clearFileSelection} disabled={selectedPaths.length === 0 || deleting || renaming || !!duplicatingFile || !!renameTarget}>Clear selection</button>{!workspace && <button className="browser-new-folder" onClick={() => setCreatingFolder(true)} disabled={folderBusy || creatingFolder}>+ New folder</button>}<button title={workspace ? 'Refresh workspace results' : 'Refresh folder'} aria-label={workspace ? 'Refresh workspace results' : 'Refresh folder'} onClick={() => workspace ? setTagRevision(value => value + 1) : browse(listing.path, selected ? { path: selected.path } : {})}><Icon name="refresh" size={17} /></button></div></div>
          {creatingFolder && <form className="browser-create-folder" onSubmit={event => { event.preventDefault(); void createMediaFolder() }}><input autoFocus aria-label="New folder name" placeholder="New folder name" value={newFolderName} onChange={event => setNewFolderName(event.target.value)} disabled={folderBusy} /><button type="submit" disabled={folderBusy || !newFolderName.trim()}>{folderBusy ? 'Creating…' : 'Create'}</button><button type="button" onClick={() => { setCreatingFolder(false); setNewFolderName('') }} disabled={folderBusy}>Cancel</button></form>}
          {duplicatingFile && <div className="file-action progress" role="status">Duplicating “{duplicatingFile}”…</div>}
          {deleting && <div className="file-action progress" role="status">Moving to Trash…</div>}
          {fileAction && <div className={`file-action ${fileAction.error ? 'error' : ''}`} role={fileAction.error ? 'alert' : 'status'}>{fileAction.message}</div>}
          {listing.catalogWarning && <div className="file-action error" role="alert">{listing.catalogWarning}</div>}
          <div className="media-view-switch" role="group" aria-label="File view">
            <label className="media-sort">Sort by <select value={mediaSort} onChange={event => { const value = event.target.value as MediaSort; setMediaSort(value); setPreference('framewise.mediaSort', value) }}><option value="name">Filename</option><option value="modified">Date modified</option></select></label>
            <label className="media-sort"><span className="sort-order-label">Order</span><select aria-label="Sort order" value={sortDirection} onChange={event => { const value = event.target.value as SortDirection; setSortDirection(value); setPreference('framewise.sortDirection', value) }}><option value="asc">Ascending</option><option value="desc">Descending</option></select></label>
            {(['list', 'grid'] as const).map(mode => <button key={mode} aria-pressed={mediaView === mode} onClick={() => { setMediaView(mode); setPreference('framewise.mediaView', mode) }}>{mode === 'list' ? 'List' : 'Grid'}</button>)}
            {mediaView === 'grid' && <label className="media-sort">Preview size <select value={gridSize} onChange={event => { const value = event.target.value as typeof gridSize; setGridSize(value); setPreference('framewise.gridSize', value) }}><option value="small">Small</option><option value="medium">Medium</option><option value="large">Large</option></select></label>}
          </div>
          <div ref={fileListRef} className={`file-list ${mediaView === 'grid' ? `media-grid grid-size-${gridSize}` : ''}`} role="group" aria-label="Files and folders" onKeyDown={navigateFiles}>
            {!workspace && listing.parent && <button className="file-row back-row" data-list-row="true" data-entry-index="-1" data-path={listing.parent} onClick={() => browse(listing.parent!, { path: listing.path })}><span className="file-icon"><Icon name="arrow" size={18} /></span><span className="file-name">Go back</span></button>}
            {mediaEntries.map((entry, index) => <button key={entry.path} data-list-row="true" data-entry-index={index} data-path={entry.path} aria-pressed={selectedPaths.includes(entry.path)} className={`file-row ${selectedPaths.includes(entry.path) ? 'selected' : ''}`} onClick={event => selectMediaEntry(entry, index, event)} onDoubleClick={() => playMediaEntry(entry)} onContextMenu={event => fileContextMenu(event, entry)}>
              {mediaView === 'grid' && !entry.isDirectory
                ? isImage(entry.path) ? <ImagePreview path={entry.path} revision={workspace ? workspaceSearch.thumbnailRevision : listing} thumbnail /> : <MediaThumbnail path={entry.path} revision={workspace ? workspaceSearch.thumbnailRevision : listing} placeholder={<Icon name="film" size={32} />} />
                : <span className={`file-icon ${entry.isDirectory ? 'folder-icon' : 'video-icon'}`}><Icon name={entry.isDirectory ? 'folder' : isImage(entry.path) ? 'image' : 'film'} size={mediaView === 'grid' ? 32 : 19} /></span>}
              <span className="file-name" title={entry.name}>{entry.name}</span>
              {workspace && <span className="result-location" title={displayPath(parentPath(entry.path))}>{displayPath(parentPath(entry.path).slice(root!.replace(/[\\/]$/, '').length).replace(/^[\\/]/, '')) || 'Workspace root'}</span>}
              <span className="file-kind">{entry.isDirectory ? 'Folder' : fileSize(entry.size)}</span>
              <Icon name="chevron" size={16} />
            </button>)}
            {resultsReady && videoCount === 0 && (workspace || tagFilter.active || listing.entries.length > 0 || !showVideos && !showImages) ? <div className="empty-list">{!showVideos && !showImages ? 'Select Videos or Images above to show files. Folders remain available.' : workspaceSearch.error && workspace ? 'Workspace results are unavailable.' : tagFilter.error ? 'Tag results are unavailable. Clear filters to browse all files.' : workspace ? 'No catalogued files match these filters in this workspace.' : 'No files match these filters in this folder.'} {tagFilter.active && <button className="clear-tag-empty" onClick={() => changeTagFilters([], tagFilter.matchAll)}>Clear filters</button>}</div> : !workspace && listing.entries.length === 0 && <div className="empty-list">No folders or supported media files here.</div>}
            {(listing.parent || listing.entries.length > 0) && <div className="file-list-tip">Tip: Enter or Space plays/pauses a video. Ctrl-click (⌘-click on Mac) or Shift-click selects several files or folders. Right-click selected files to edit their tags together. A normal click opens a folder.</div>}
          </div>
          {workspace && workspaceSearch.total > 200 && <div className="workspace-pagination" aria-label="Workspace result pages"><button disabled={!resultsReady || workspaceSearch.page === 0} onClick={() => changeSearchPage(workspaceSearch.page - 1)}>Previous</button><span>Page {workspaceSearch.page + 1} of {Math.ceil(workspaceSearch.total / 200)} · {workspaceSearch.total} results</span><button disabled={!resultsReady || (workspaceSearch.page + 1) * 200 >= workspaceSearch.total} onClick={() => changeSearchPage(workspaceSearch.page + 1)}>Next</button></div>}
        </>}
        </>}
      </main>

      {view === 'media' && <section className={`details-panel ${inspectorCollapsed ? 'collapsed' : ''}`} aria-label="Media metadata">
        <div className="details-top">
          {!inspectorCollapsed && <div><div className="eyebrow">INSPECTOR</div><h2>File details</h2></div>}
          <div className="details-actions">{!inspectorCollapsed && probe && <button className="icon-button" onClick={copyMetadata} title="Copy raw metadata" aria-label="Copy raw metadata"><Icon name={copyDone ? 'check' : 'copy'} size={17} /></button>}
          <button className="panel-toggle inspector-toggle" onClick={toggleInspector} aria-label={inspectorCollapsed ? 'Expand inspector' : 'Collapse inspector'} aria-expanded={!inspectorCollapsed} title={inspectorCollapsed ? 'Expand inspector' : 'Collapse inspector'}><Icon name="chevron" size={17} /></button></div>
        </div>
        {!inspectorCollapsed && (selected && resultsReady && mediaEntries.some(entry => entry.path === selected.path) ? <div key={selected.path} className="details-body">
          {selectedImage ? <ImagePreview path={selected.path} revision={selected} /> : <VideoPreview file={selected} playbackAction={playbackRequest?.path === selected.path ? playbackRequest.action : null} onPlayRequestHandled={() => setPlaybackRequest(null)} />}
          <div className="selected-file"><span className="selected-file-icon"><Icon name={selectedImage ? 'image' : 'film'} size={27} /></span><div><strong title={selected.name}>{selected.name}</strong><span>{fileSize(selected.size)}</span></div></div>
          <dl className="property-list"><Property label="Modified" value={selected.modifiedAt === null ? 'Unavailable' : new Date(selected.modifiedAt).toLocaleString()} /><Property label="Created" value={selected.createdAt == null ? 'Unavailable' : new Date(selected.createdAt).toLocaleString()} /></dl>
          {probe && video && !selectedImage && <button className="inspector-edit" disabled={scanRunning} onClick={() => editFile(selected)}>Edit video</button>}
          <section aria-label="Media tags"><div className="section-title">TAGS</div>{selected.videoId
            ? <VideoTags key={`${selected.path}:${selected.videoId}`} videos={[{ videoId: selected.videoId, path: selected.path }]} refreshToken={tagRevision} onChanged={() => setTagRevision(value => value + 1)} />
            : <p className="no-data">Refresh the folder to load this file's organization record before assigning tags.</p>}</section>
          {loading && <div className="notice">Reading media metadata…</div>}
          {error && <div className="notice error" role="alert"><Icon name="info" size={18} /><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss error"><Icon name="close" size={15} /></button></div>}
          {probe && <>
            <GenerationPrompt probe={probe} image={selectedImage} />
            <div className="section-title">OVERVIEW</div>
            <dl className="property-list">{!selectedImage && <Property label="Duration" value={duration(probe.format?.duration)} />}<Property label="File size" value={fileSize(probe.format?.size ?? selected.size)} /><Property label={selectedImage ? 'Format' : 'Container'} value={probe.format?.format_long_name ?? probe.format?.format_name} />{!selectedImage && <Property label="Bitrate" value={bitrate(probe.format?.bit_rate)} />}</dl>
            <div className="section-title">{selectedImage ? 'IMAGE DETAILS' : 'VIDEO STREAM'}</div>
            {video ? <dl className="property-list"><Property label="Codec" value={video.codec_name?.toUpperCase()} /><Property label="Resolution" value={video.width && video.height ? `${video.width} × ${video.height}` : undefined} />{!selectedImage && <Property label="Frame rate" value={frameRate(video.avg_frame_rate ?? video.r_frame_rate)} />}<Property label="Pixel format" value={video.pix_fmt} /><Property label="Color space" value={video.color_space} /></dl> : <p className="no-data">No stream information found.</p>}
            {!selectedImage && <>
            <div className="section-title">AUDIO STREAM</div>
            {audio ? <dl className="property-list"><Property label="Codec" value={audio.codec_name?.toUpperCase()} /><Property label="Channels" value={audio.channel_layout ?? audio.channels} /><Property label="Sample rate" value={audio.sample_rate ? `${Number(audio.sample_rate).toLocaleString()} Hz` : undefined} /></dl> : <p className="no-data">No audio stream found.</p>}
            </>}
            <div className="section-title">EMBEDDED TAGS</div>
            <EmbeddedTags tags={probe.format?.tags ?? {}} />
            <details className="raw-details"><summary>View raw metadata</summary><pre>{JSON.stringify(probe, null, 2)}</pre></details>
          </>}
        </div> : <div className="details-empty"><div className="details-empty-icon"><Icon name="info" size={27} /></div><h3>Nothing selected</h3><p>Choose a media file from the browser to see its metadata here.</p>{error && <div className="notice error" role="alert">{error}</div>}</div>)}
      </section>}
      {!workspaceCollapsed && <div className="column-resizer left-resizer" role="separator" tabIndex={0} aria-label="Resize workspace column" aria-orientation="vertical" aria-valuemin={MIN_WORKSPACE_WIDTH} aria-valuemax={widthLimits('left').max} aria-valuenow={effectiveWorkspaceWidth} onPointerDown={event => startResize('left', event)} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize} onKeyDown={event => resizeWithKeyboard('left', event)} />}
      {view === 'media' && !inspectorCollapsed && <div className="column-resizer right-resizer" role="separator" tabIndex={0} aria-label="Resize inspector column" aria-orientation="vertical" aria-valuemin={MIN_INSPECTOR_WIDTH} aria-valuemax={widthLimits('right').max} aria-valuenow={effectiveInspectorWidth} onPointerDown={event => startResize('right', event)} onPointerMove={moveResize} onPointerUp={endResize} onPointerCancel={endResize} onKeyDown={event => resizeWithKeyboard('right', event)} />}
    </div>
    {contextMenu && <div ref={contextMenuRef} className="file-context-menu" role="menu" aria-label={`Actions for ${contextMenu.file.name}`} style={{ left: contextMenu.x, top: contextMenu.y }}>
      {workspace && <button role="menuitem" onClick={() => { setContextMenu(null); void browse(parentPath(contextMenu.file.path), { path: contextMenu.file.path }) }}>Open containing folder</button>}
      <button className="reveal-menu-item" role="menuitem" onClick={() => void showInFileManager(contextMenu.file)}>{/Win/i.test(navigator.platform) ? 'Show in Explorer' : /Mac/i.test(navigator.platform) ? 'Show in Finder' : 'Show in File Manager'}</button>
      {contextMenu.file.isDirectory ? <>
        <button className="rename-menu-item" role="menuitem" disabled={scanRunning} onClick={() => openRename(contextMenu.file)}>Rename folder</button>
        <button className="move-menu-item" role="menuitem" disabled={scanRunning} onClick={() => void openMoveWindow(contextMenu.file)}>Move to…{selectedPaths.includes(contextMenu.file.path) && selectedPaths.length > 1 ? ` (${selectedPaths.length})` : ''}</button>
        <button role="menuitem" disabled={scanRunning} onClick={() => void moveEntryToTrash(contextMenu.file)}>Move folder to Trash</button>
      </> : <>
        {!isImage(contextMenu.file.path) && <button className="edit-menu-item" role="menuitem" disabled={scanRunning} onClick={() => editFile(contextMenu.file)}>Edit video</button>}
        <button className="edit-menu-item" role="menuitem" disabled={scanRunning} onClick={() => openVideoTags(contextMenu.file)}>Edit tags…{selectedPaths.includes(contextMenu.file.path) && selectedPaths.length > 1 ? ` (${selectedPaths.length})` : ''}</button>
        <button className="rename-menu-item" role="menuitem" disabled={scanRunning} onClick={() => openRename(contextMenu.file)}>Rename</button>
        <button className="move-menu-item" role="menuitem" disabled={scanRunning} onClick={() => void openMoveWindow(contextMenu.file)}>Move to…{selectedPaths.includes(contextMenu.file.path) && selectedPaths.length > 1 ? ` (${selectedPaths.length})` : ''}</button>
        <button className="duplicate-menu-item" role="menuitem" disabled={scanRunning} onClick={() => void duplicateFile(contextMenu.file)}>Duplicate</button>
        <button role="menuitem" disabled={scanRunning} onClick={() => void moveEntryToTrash(contextMenu.file)}>{selectedPaths.includes(contextMenu.file.path) && selectedPaths.length > 1 ? `Move ${selectedPaths.length} files to Trash` : 'Move to Trash'}</button>
      </>}
    </div>}
    {tagBatch && <TagBatchDialog videos={tagBatch} onChanged={() => setTagRevision(value => value + 1)} onClose={closeVideoTags} />}
    {renameTarget && <div className="rename-backdrop">
      <form className="rename-dialog" role="dialog" aria-modal="true" aria-labelledby="rename-title" onSubmit={event => { event.preventDefault(); void renameFile() }} onKeyDown={event => { if (event.key === 'Escape') { event.preventDefault(); closeRename() } }}>
        <h2 id="rename-title">Rename {renameTarget.isDirectory ? 'folder' : isImage(renameTarget.path) ? 'image' : 'video'}</h2>
        <label htmlFor="rename-name">{renameTarget.isDirectory ? 'Folder name' : 'File name'}</label>
        <div className="rename-name-field"><input id="rename-name" autoFocus value={renameStem} onFocus={event => event.currentTarget.select()} onChange={event => setRenameStem(event.target.value)} disabled={renaming} />{!renameTarget.isDirectory && <span>{fixedExtension(renameTarget)}</span>}</div>
        {renameError && <p className="rename-error" role="alert">{renameError}</p>}
        <div className="rename-actions"><button type="button" onClick={closeRename} disabled={renaming}>Cancel</button><button type="submit" disabled={renaming || !renameStem.trim() || renameStem === editableName(renameTarget)}>{renaming ? 'Renaming…' : 'Rename'}</button></div>
      </form>
    </div>}
  </div>
}

export default App
