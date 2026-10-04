import { useCallback, useEffect, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { ask, save as saveDialog } from '@tauri-apps/plugin-dialog'
import { error as logError } from '@tauri-apps/plugin-log'
import { cutPreviewAction } from './cutPreview'
import { adjacentFrameTime, frameIndexAt } from './frameNavigation'
import { displayPath, versionedMediaSrc } from './paths'
import './editor.css'

type EditorFile = { name: string; path: string }
type EditSource = { videoPath: string; duration: number; sourceSignature: string }
export type EditResult = { outputPath: string; backupPath: string | null; metadataWarnings: string[] }
type EditProgress = { source: string; percent: number }

function timecode(seconds: number) {
  const safe = Math.max(0, seconds)
  const minutes = Math.floor(safe / 60)
  return `${String(minutes).padStart(2, '0')}:${(safe % 60).toFixed(2).padStart(5, '0')}`
}

function errorText(error: unknown) { return error instanceof Error ? error.message : String(error) }
function reportError(context: string, cause: unknown) {
  const message = errorText(cause)
  if (isTauri()) void logError(`${context}: ${message}`).catch(() => {})
  return message
}

export default function Editor({ file, onExit, onSaved }: { file: EditorFile; onExit: (result: EditResult | null, replace: boolean) => void; onSaved: (result: EditResult) => void }) {
  const [source, setSource] = useState<EditSource | null>(null)
  const [frameTimes, setFrameTimes] = useState<number[] | null>(null)
  const [frameError, setFrameError] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [savedResult, setSavedResult] = useState<EditResult | null>(null)
  const [videoMounted, setVideoMounted] = useState(true)
  const [playhead, setPlayhead] = useState(0)
  const [mediaDuration, setMediaDuration] = useState<number | null>(null)
  const [cutStart, setCutStart] = useState<number | null>(null)
  const [cutEnd, setCutEnd] = useState<number | null>(null)
  const [cutApplied, setCutApplied] = useState(false)
  const [hasUnsavedChanges, setHasUnsavedChanges] = useState(false)
  const [playing, setPlaying] = useState(false)
  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState(0)
  const videoRef = useRef<HTMLVideoElement | null>(null)
  const cutSkipArmed = useRef(false)

  useEffect(() => {
    let active = true
    invoke<EditSource>('prepare_edit', { path: file.path })
      .then(value => { if (active) setSource(value) })
      .catch(cause => { if (active) setError(reportError('Opening video editor', cause)) })
    return () => { active = false }
  }, [file.path])

  useEffect(() => {
    if (!source) return
    let active = true
    invoke<number[]>('video_frame_times', { path: file.path, sourceSignature: source.sourceSignature })
      .then(times => { if (active) setFrameTimes(times) })
      .catch(cause => { if (active) setFrameError(reportError('Reading video frame times', cause)) })
    return () => { active = false }
  }, [file.path, source])

  const duration = mediaDuration ?? source?.duration ?? 0
  const lastFrameTime = frameTimes?.at(-1) ?? 0
  const validCut = cutStart !== null && cutEnd !== null && cutEnd - cutStart >= 0.01 && duration - (cutEnd - cutStart) >= 0.2

  const syncPreviewTime = useCallback(() => {
    const video = videoRef.current
    if (!video || video.seeking) return
    const time = video.currentTime
    if (cutApplied && cutStart !== null && cutEnd !== null && !video.paused) {
      const action = cutPreviewAction(time, cutStart, cutEnd, duration, cutSkipArmed.current)
      if (action === 'stop') {
        cutSkipArmed.current = false
        video.pause()
        video.currentTime = 0
        setPlayhead(0)
        return
      }
      if (action === 'skip') {
        cutSkipArmed.current = false
        video.currentTime = cutEnd
        setPlayhead(cutEnd)
        return
      }
    }
    setPlayhead(time)
  }, [cutApplied, cutStart, cutEnd, duration])

  function previewSeeked() {
    const video = videoRef.current
    cutSkipArmed.current = video !== null && cutEnd !== null && video.currentTime < cutEnd - 0.0001
    syncPreviewTime()
  }

  function syncMediaDuration() {
    const value = videoRef.current?.duration
    if (value !== undefined && Number.isFinite(value) && value > 0) setMediaDuration(value)
  }

  useEffect(() => {
    if (!playing || !cutApplied) return
    let frame = 0
    const check = () => {
      syncPreviewTime()
      frame = window.requestAnimationFrame(check)
    }
    frame = window.requestAnimationFrame(check)
    return () => window.cancelAnimationFrame(frame)
  }, [playing, cutApplied, syncPreviewTime])

  function seek(value: number) {
    const next = Math.min(Math.max(value, 0), duration)
    cutSkipArmed.current = cutEnd !== null && next < cutEnd - 0.0001
    if (videoRef.current) videoRef.current.currentTime = next
    setPlayhead(next)
  }

  function updateStart(value: number | null) {
    const next = value === null ? null : Math.min(Math.max(value, 0), duration)
    setCutStart(next)
    setCutApplied(false)
    setHasUnsavedChanges(next !== null || cutEnd !== null)
    setSavedResult(null)
  }

  function updateEnd(value: number | null) {
    const next = value === null ? null : Math.min(Math.max(value, 0), duration)
    setCutEnd(next)
    setCutApplied(false)
    setHasUnsavedChanges(cutStart !== null || next !== null)
    setSavedResult(null)
  }

  function startPlayback() {
    if (videoRef.current) void videoRef.current.play().catch(cause => setError(reportError('Playing video', cause)))
  }

  function togglePlayback() {
    const video = videoRef.current
    if (!video) return
    if (!video.paused) { video.pause(); return }
    if (video.ended || video.currentTime >= duration - 0.01) seek(0)
    startPlayback()
  }

  function playAgain() {
    seek(0)
    startPlayback()
  }

  function stepFrame(direction: -1 | 1) {
    const video = videoRef.current
    if (!video || !frameTimes?.length) return
    video.pause()
    const next = adjacentFrameTime(frameTimes, playhead, direction)
    if (next !== null) seek(next)
  }

  function applyCut() {
    if (!validCut) return
    const video = videoRef.current
    video?.pause()
    setCutApplied(true)
    seek(Math.max(0, (cutStart ?? 0) - 1))
    if (video) startPlayback()
  }

  function undoCut() {
    cutSkipArmed.current = false
    setCutApplied(false)
    setCutStart(null)
    setCutEnd(null)
    setHasUnsavedChanges(false)
    setSavedResult(null)
  }

  async function leave() {
    if (busy) return
    if (hasUnsavedChanges) {
      try {
        if (!await ask('Discard your unsaved edit?', { title: 'Leave editor', kind: 'warning', okLabel: 'Discard edit', cancelLabel: 'Keep editing' })) return
      } catch (cause) { setError(reportError('Leaving video editor', cause)); return }
    }
    onExit(null, false)
  }

  async function exportVideo(replace: boolean) {
    if (!source || !validCut || !cutApplied || cutStart === null || cutEnd === null || busy) return
    setError(null)
    let destination = file.path
    try {
      if (replace) {
        if (!await ask(`Replace “${file.name}” with the edited video? The original will be moved to Trash after the new file is verified.`, { title: 'Replace original video', kind: 'warning', okLabel: 'Replace video', cancelLabel: 'Cancel' })) return
      } else {
        const suggested = displayPath(file.path).replace(/\.[^./\\]+$/, '') + '-edited.mp4'
        const chosen = await saveDialog({ title: 'Save edited video as MP4', defaultPath: suggested, filters: [{ name: 'MP4 video', extensions: ['mp4'] }] })
        if (!chosen) return
        destination = chosen
      }
      setBusy(true)
      setSavedResult(null)
      setProgress(0)
      const unlisten = await listen<EditProgress>('edit-progress', event => {
        if (event.payload.source === source.videoPath) setProgress(event.payload.percent)
      })
      try {
        videoRef.current?.pause()
        flushSync(() => setVideoMounted(false))
        const result = await invoke<EditResult>('export_edit', { request: {
          path: file.path, destination, sourceSignature: source.sourceSignature,
          start: cutStart, end: cutEnd, replace,
        } })
        if (replace) onExit(result, true)
        else {
          cutSkipArmed.current = cutApplied && cutEnd !== null
          setVideoMounted(true)
          setPlayhead(0)
          setPlaying(false)
          setBusy(false)
          setHasUnsavedChanges(false)
          setSavedResult(result)
          onSaved(result)
        }
      } finally { unlisten() }
    } catch (cause) {
      setError(reportError('Saving edited video', cause))
      cutSkipArmed.current = cutApplied && cutEnd !== null
      setVideoMounted(true)
      setBusy(false)
    }
  }

  return <div className="editor-screen">
    <header className="editor-header">
      <button className="editor-back" onClick={() => void leave()} disabled={busy}>← Back to media</button>
      <div className="editor-heading"><span className="eyebrow">VIDEO EDITOR</span><h1 title={file.name}>{file.name}</h1></div>
      <div className="editor-save-actions">
        <button className="editor-save-as" onClick={() => void exportVideo(false)} disabled={!cutApplied || busy}>Save As…</button>
        <button className="editor-save" onClick={() => void exportVideo(true)} disabled={!cutApplied || busy || !file.name.toLowerCase().endsWith('.mp4')} title={!file.name.toLowerCase().endsWith('.mp4') ? 'Use Save As to create an MP4' : undefined}>Save</button>
      </div>
    </header>

    <main className="editor-main">
      <div className="editor-video-wrap">
        {source && videoMounted ? <video ref={videoRef} src={versionedMediaSrc(source.videoPath, source.sourceSignature)} controls playsInline preload="metadata" onLoadedMetadata={() => { syncMediaDuration(); cutSkipArmed.current = cutApplied && cutEnd !== null && (videoRef.current?.currentTime ?? Infinity) < cutEnd }} onDurationChange={syncMediaDuration} onTimeUpdate={syncPreviewTime} onSeeked={previewSeeked} onPlay={() => { setPlaying(true); syncPreviewTime() }} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)} aria-label={`Preview ${file.name}`} /> : <div className="editor-video-placeholder">{busy ? 'Rendering your edited video…' : source ? 'Preview paused' : error ? 'Editor unavailable for this video' : 'Opening video…'}</div>}
      </div>
      {savedResult && <div className="editor-saved" role="status">Saved as {displayPath(savedResult.outputPath)}. Continue editing the original video here.{savedResult.metadataWarnings.length > 0 && <> Track metadata changed: {savedResult.metadataWarnings.join('; ')}.</>}</div>}
      {error && <div className="editor-error" role="alert">{error}</div>}
      {busy && <div className="editor-progress" role="status"><span>Rendering MP4… {Math.round(progress)}%</span><progress max="100" value={progress} /></div>}
      {source && <section className="editor-controls" aria-label="Edit controls">
        <div className="editor-time-line"><strong>{timecode(playhead)}</strong><span>of {timecode(duration)}</span>{frameTimes?.length ? <span className="editor-frame-position">Frame {frameIndexAt(frameTimes, playhead) + 1} of {frameTimes.length}</span> : <span className="editor-frame-position">{frameError ? 'Frame navigation unavailable' : 'Reading frame times…'}</span>}</div>
        <div className="editor-track-wrap">
          {cutStart !== null && cutEnd !== null && cutEnd > cutStart && <div className={`editor-cut-overlay ${cutApplied ? 'applied' : ''}`} style={{ left: `${100 * cutStart / duration}%`, width: `${100 * (cutEnd - cutStart) / duration}%` }} />}
          <input type="range" min="0" max={duration} step="any" value={Math.min(playhead, duration)} onChange={event => seek(Number(event.target.value))} aria-label="Scrub through video" disabled={busy} />
        </div>
        <div className="editor-playback-controls" role="group" aria-label="Playback controls">
          <button type="button" onClick={playAgain} disabled={busy}>Play again</button>
          <button type="button" onClick={() => seek(0)} disabled={busy}>Move to start</button>
          <button type="button" onClick={() => stepFrame(-1)} disabled={busy || !frameTimes?.length || playhead <= (frameTimes[0] ?? 0) + 0.0001} title={frameError ?? undefined} aria-label="Back one frame">← 1 frame</button>
          <button type="button" onClick={() => stepFrame(1)} disabled={busy || !frameTimes?.length || playhead >= lastFrameTime - 0.0001} title={frameError ?? undefined} aria-label="Forward one frame">1 frame →</button>
          <button type="button" className="editor-play" onClick={togglePlayback} disabled={busy}>{playing ? 'Pause' : 'Play'}</button>
        </div>
        <div className="editor-range-actions">
          <div className="editor-marker"><button onClick={() => updateStart(playhead)} disabled={busy}>Mark start</button><label>Start (seconds)<input type="number" min="0" max={duration} step="0.01" value={cutStart ?? ''} placeholder="—" onChange={event => updateStart(event.target.value === '' ? null : Number(event.target.value))} disabled={busy} /></label>{cutStart !== null && <span>{timecode(cutStart)}</span>}</div>
          <div className="editor-marker"><button onClick={() => updateEnd(playhead)} disabled={busy}>Mark end</button><label>End (seconds)<input type="number" min="0" max={duration} step="0.01" value={cutEnd ?? ''} placeholder="—" onChange={event => updateEnd(event.target.value === '' ? null : Number(event.target.value))} disabled={busy} /></label>{cutEnd !== null && <span>{timecode(cutEnd)}</span>}</div>
        </div>
        <div className="editor-cut-actions">
          <button className="editor-remove" onClick={applyCut} disabled={!validCut || cutApplied || busy}>{cutApplied ? 'Removal ready' : 'Remove marked section'}</button>
          <button className="editor-undo" onClick={undoCut} disabled={!cutApplied || busy}>Undo removal</button>
        </div>
        {cutApplied && cutStart !== null && cutEnd !== null
          ? <div className="editor-cut-ready" role="status"><strong>✓ Removal ready</strong><span>{timecode(cutEnd - cutStart)} is marked for removal. Playback skips it. Choose Save or Save As to create the edited video.</span></div>
          : <p className="editor-cut-hint">Mark a start and end, then remove that section.</p>}
        <p className="editor-note">The original stays unchanged until Save. Output is 8-bit H.264/AAC MP4 with one video track and up to one audio track. Descriptive tags are copied where supported; duration and frame counts are recalculated.</p>
      </section>}
    </main>
  </div>
}
