import { useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import './App.css'

type FileEntry = { name: string; path: string; isDirectory: boolean; size: number | null }
type DirectoryListing = { path: string; parent: string | null; entries: FileEntry[] }
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

function Icon({ name, size = 20 }: { name: 'folder' | 'film' | 'chevron' | 'arrow' | 'info' | 'refresh' | 'copy' | 'check' | 'close'; size?: number }) {
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
function display(value: unknown) { return value === undefined || value === null || value === '' ? '—' : String(value) }

function Property({ label, value }: { label: string; value: unknown }) {
  return <div className="property"><dt>{label}</dt><dd>{display(value)}</dd></div>
}

function App() {
  const [root, setRoot] = useState<string | null>(null)
  const [listing, setListing] = useState<DirectoryListing | null>(null)
  const [selected, setSelected] = useState<FileEntry | null>(null)
  const [probe, setProbe] = useState<Probe | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [copyDone, setCopyDone] = useState(false)
  const requestId = useRef(0)

  async function chooseFolder() {
    try {
      const path = await open({ directory: true, multiple: false, title: 'Choose a video folder' })
      if (!path || Array.isArray(path)) return
      const next = await invoke<DirectoryListing>('select_root', { path })
      requestId.current++
      setRoot(next.path); setListing(next); setSelected(null); setProbe(null); setError(null)
    } catch (cause) { setError(errorText(cause)) }
  }

  async function browse(path: string) {
    try {
      const next = await invoke<DirectoryListing>('list_directory', { path })
      requestId.current++
      setListing(next); setSelected(null); setProbe(null); setError(null); setLoading(false)
    } catch (cause) { setError(errorText(cause)) }
  }

  async function selectFile(file: FileEntry) {
    const currentRequest = ++requestId.current
    setSelected(file); setProbe(null); setError(null); setLoading(true)
    try {
      const result = await invoke<Probe>('inspect_video', { path: file.path })
      if (currentRequest === requestId.current) setProbe(result)
    } catch (cause) {
      if (currentRequest === requestId.current) setError(errorText(cause))
    } finally {
      if (currentRequest === requestId.current) setLoading(false)
    }
  }

  async function copyMetadata() {
    if (!probe) return
    try {
      await navigator.clipboard.writeText(JSON.stringify(probe, null, 2))
      setCopyDone(true)
      window.setTimeout(() => setCopyDone(false), 1800)
    } catch (cause) { setError(`Could not copy metadata: ${errorText(cause)}`) }
  }

  const video = probe?.streams?.find(stream => stream.codec_type === 'video')
  const audio = probe?.streams?.find(stream => stream.codec_type === 'audio')
  const pathParts = listing?.path.split(/[\\/]/).filter(Boolean) ?? []
  const directoryCount = listing?.entries.filter(entry => entry.isDirectory).length ?? 0
  const videoCount = listing?.entries.length ? listing.entries.length - directoryCount : 0

  return <div className="app-shell">
    <header className="topbar">
      <div className="brand"><div className="brand-mark"><Icon name="film" size={21} /></div><span>Framewise</span><span className="brand-beta">BETA</span></div>
      <span className="topbar-note">Local video inspector</span>
      <button className="choose-button" onClick={chooseFolder}><Icon name="folder" size={17} /> Choose folder</button>
    </header>

    <div className="workspace">
      <aside className="sidebar">
        <div className="sidebar-heading">WORKSPACE</div>
        {root ? <>
          <button className="root-item" onClick={() => browse(root)} title={root}><Icon name="folder" size={19} /><span>{root.split(/[\\/]/).filter(Boolean).at(-1) || root}</span></button>
          <div className="sidebar-section-label">CURRENT FOLDER</div>
          <div className="sidebar-current" title={listing?.path}>{listing?.path}</div>
        </> : <div className="sidebar-hint">Choose a folder to see your videos here.</div>}
        <div className="sidebar-bottom"><span className="status-dot" /> Files stay on your device</div>
      </aside>

      <main className="main-panel">
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
          <div className="breadcrumb"><button onClick={() => root && browse(root)}>{root?.split(/[\\/]/).filter(Boolean).at(-1) || 'Root'}</button>{listing.path !== root && <><Icon name="chevron" size={14} /><span>{pathParts.at(-1)}</span></>}</div>
          <div className="browser-toolbar"><span>{directoryCount} {directoryCount === 1 ? 'folder' : 'folders'} <span className="dot-separator">·</span> {videoCount} {videoCount === 1 ? 'video' : 'videos'}</span><button title="Refresh folder" aria-label="Refresh folder" onClick={() => browse(listing.path)}><Icon name="refresh" size={17} /></button></div>
          <div className="file-list">
            {listing.parent && <button className="file-row back-row" onClick={() => browse(listing.parent!)}><span className="file-icon"><Icon name="arrow" size={18} /></span><span className="file-name">Go back</span></button>}
            {listing.entries.map(entry => <button key={entry.path} className={`file-row ${selected?.path === entry.path ? 'selected' : ''}`} onClick={() => entry.isDirectory ? browse(entry.path) : selectFile(entry)}>
              <span className={`file-icon ${entry.isDirectory ? 'folder-icon' : 'video-icon'}`}><Icon name={entry.isDirectory ? 'folder' : 'film'} size={19} /></span>
              <span className="file-name" title={entry.name}>{entry.name}</span>
              <span className="file-kind">{entry.isDirectory ? 'Folder' : fileSize(entry.size)}</span>
              <Icon name="chevron" size={16} />
            </button>)}
            {listing.entries.length === 0 && <div className="empty-list">No folders or supported video files here.</div>}
          </div>
        </>}
      </main>

      <section className="details-panel" aria-label="Video metadata">
        <div className="details-top"><div><div className="eyebrow">INSPECTOR</div><h2>File details</h2></div>{probe && <button className="icon-button" onClick={copyMetadata} title="Copy raw metadata" aria-label="Copy raw metadata"><Icon name={copyDone ? 'check' : 'copy'} size={17} /></button>}</div>
        {selected ? <div className="details-body">
          <div className="selected-file"><span className="selected-file-icon"><Icon name="film" size={27} /></span><div><strong title={selected.name}>{selected.name}</strong><span>{fileSize(selected.size)}</span></div></div>
          {loading && <div className="notice">Reading video metadata…</div>}
          {error && <div className="notice error" role="alert"><Icon name="info" size={18} /><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss error"><Icon name="close" size={15} /></button></div>}
          {probe && <>
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
        </div> : <div className="details-empty"><div className="details-empty-icon"><Icon name="info" size={27} /></div><h3>Nothing selected</h3><p>Choose a video from the browser to see its metadata here.</p>{error && <div className="notice error" role="alert">{error}</div>}</div>}
      </section>
    </div>
  </div>
}

export default App
