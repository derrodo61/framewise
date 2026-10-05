import { invoke, isTauri } from '@tauri-apps/api/core'
import { error as logError } from '@tauri-apps/plugin-log'

const keys = [
  'framewise.defaultFolder',
  'framewise.theme',
  'framewise.mediaView',
  'framewise.gridSize',
  'framewise.mediaSort',
  'framewise.sortDirection',
  'framewise.workspaceCollapsed',
  'framewise.inspectorCollapsed',
  'framewise.workspaceWidth',
  'framewise.inspectorWidth',
  'framewise.settingsUpdatedAt',
] as const

type PreferenceKey = typeof keys[number]
const updatedAtKey = 'framewise.settingsUpdatedAt'
let pendingSave: Promise<void> = Promise.resolve()

function snapshot(): Record<string, string> {
  return Object.fromEntries(keys.flatMap(key => {
    const value = window.localStorage.getItem(key)
    return value === null ? [] : [[key, value]]
  }))
}

function saveNative() {
  if (!isTauri()) return
  const values = snapshot()
  pendingSave = pendingSave.catch(() => {}).then(() => invoke<void>('save_preferences', { values }))
    .catch(cause => { void logError(`Could not save preferences: ${String(cause)}`).catch(() => {}) })
}

export async function initializePreferences(saveIfMissing = true) {
  if (!isTauri()) return
  try {
    const saved = await invoke<Record<string, string> | null>('load_preferences')
    if (saved) {
      const localUpdatedAt = Number(window.localStorage.getItem(updatedAtKey)) || 0
      const nativeUpdatedAt = Number(saved[updatedAtKey]) || 0
      if (localUpdatedAt > 0 && localUpdatedAt >= nativeUpdatedAt) {
        await invoke<void>('save_preferences', { values: snapshot() })
      } else {
        for (const key of keys) {
          if (Object.hasOwn(saved, key)) window.localStorage.setItem(key, saved[key])
          else window.localStorage.removeItem(key)
        }
      }
    } else if (saveIfMissing && keys.some(key => key !== updatedAtKey && window.localStorage.getItem(key) !== null)) {
      window.localStorage.setItem(updatedAtKey, String(Date.now()))
      await invoke<void>('save_preferences', { values: snapshot() })
    }
  } catch (cause) {
    void logError(`Could not load preferences: ${String(cause)}`).catch(() => {})
  }
}

export function getPreference(key: PreferenceKey) { return window.localStorage.getItem(key) }
function markUpdated() {
  const previous = Number(window.localStorage.getItem(updatedAtKey)) || 0
  window.localStorage.setItem(updatedAtKey, String(Math.max(Date.now(), previous + 1)))
}

export function setPreference(key: PreferenceKey, value: string) { window.localStorage.setItem(key, value); markUpdated(); saveNative() }
export function removePreference(key: PreferenceKey) { window.localStorage.removeItem(key); markUpdated(); saveNative() }
