import { invoke } from '@tauri-apps/api/core'

type Job = {
  path: string
  consumers: Array<() => boolean>
  resolve: (path: string | null) => void
  reject: (cause: unknown) => void
}
const pending: Job[] = []
const requests = new Map<string, { promise: Promise<string | null>; job: Job }>()
let running = 0

function drain() {
  while (running < 2 && pending.length) {
    const job = pending.shift()!
    if (!job.consumers.some(active => active())) {
      requests.delete(job.path)
      job.resolve(null)
      continue
    }
    running++
    void invoke<string | null>('generate_preview_thumbnail', { path: job.path })
      .then(job.resolve, job.reject)
      .finally(() => { running--; requests.delete(job.path); drain() })
  }
}

// Share generation with the inspector and skip queued cards that left the viewport.
export function generateThumbnail(path: string, active: () => boolean, priority = false): Promise<string | null> {
  const existing = requests.get(path)
  if (existing) {
    existing.job.consumers.push(active)
    const index = pending.indexOf(existing.job)
    if (priority && index > 0) { pending.splice(index, 1); pending.unshift(existing.job) }
    return existing.promise
  }
  let job!: Job
  const promise = new Promise<string | null>((resolve, reject) => {
    job = { path, consumers: [active], resolve, reject }
  })
  requests.set(path, { promise, job })
  if (priority) pending.unshift(job)
  else pending.push(job)
  drain()
  return promise
}
