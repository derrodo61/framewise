import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { isTauri } from '@tauri-apps/api/core'
import { error as logError } from '@tauri-apps/plugin-log'
import './index.css'
import App from './App.tsx'

document.documentElement.dataset.theme = window.localStorage.getItem('framewise.theme') === 'dark' ? 'dark' : 'light'

function logUnhandled(message: string) {
  if (isTauri()) void logError(message).catch(() => {})
}

window.addEventListener('error', event => logUnhandled(`Uncaught UI error: ${event.message}`))
window.addEventListener('unhandledrejection', event => logUnhandled(`Unhandled UI promise rejection: ${String(event.reason)}`))

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
