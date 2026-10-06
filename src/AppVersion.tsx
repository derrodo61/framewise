import { version } from '../package.json'
import './app-version.css'

export default function AppVersion() {
  return <span className="app-version" aria-label={`Version ${version}`} title={`Framewise ${version}`}>v{version}</span>
}
