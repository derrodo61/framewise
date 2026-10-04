import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { isTauri } from '@tauri-apps/api/core'
import { error as logError } from '@tauri-apps/plugin-log'
import './index.css'
import App from './App.tsx'
import MoveWindow from './MoveWindow.tsx'
import { getPreference, initializePreferences } from './preferences'

function logUnhandled(message: string) {
  if (isTauri()) void logError(message).catch(() => {})
}

window.addEventListener('error', event => logUnhandled(`Uncaught UI error: ${event.message}`))
window.addEventListener('unhandledrejection', event => logUnhandled(`Unhandled UI promise rejection: ${String(event.reason)}`))

const isMoveWindow = new URLSearchParams(window.location.search).has('moveTo')
void initializePreferences(!isMoveWindow).then(() => {
  document.documentElement.dataset.theme = getPreference('framewise.theme') === 'dark' ? 'dark' : 'light'
  createRoot(document.getElementById('root')!).render(
    <StrictMode>{isMoveWindow ? <MoveWindow /> : <App />}</StrictMode>,
  )
})
