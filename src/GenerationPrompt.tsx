import { useMemo, useState } from 'react'
import { extractGenerationMetadata } from './generationMetadata'
import type { ExtractedPrompt, ExtractedSeed } from './generationMetadata'
import './generation-prompt.css'

function PromptText({ prompt, index }: { prompt: ExtractedPrompt; index: number }) {
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState<string | null>(null)
  async function copy() {
    try { await navigator.clipboard.writeText(prompt.text); setCopied(true); setError(null) }
    catch { setError('Could not copy. You can select and copy the text below.') }
  }
  const title = prompt.kind === 'negative' ? 'Negative prompt' : 'Positive prompt'
  return <div className="generation-prompt-text">
    <div className="generation-prompt-heading"><strong>{title}</strong><button onClick={() => void copy()} aria-label={`Copy ${title.toLowerCase()} ${index + 1}`}>{copied ? 'Copied' : 'Copy prompt'}</button></div>
    <pre tabIndex={0} aria-label={title}>{prompt.text}</pre>
    <span className="generation-prompt-source">{prompt.source}</span>
    {error && <p role="alert">{error}</p>}
  </div>
}
function SeedValue({ seed, index, image }: { seed: ExtractedSeed; index: number; image: boolean }) {
  const [copied, setCopied] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const title = seed.kind === 'video' ? image ? 'Image seed' : 'Video seed' : seed.kind === 'prompt' ? 'Prompt generation seed' : 'Seed'
  async function copy() {
    try { await navigator.clipboard.writeText(seed.value); setCopied(true); setError(null) }
    catch { setError('Could not copy. You can select and copy the seed below.') }
  }
  return <div className="generation-prompt-text generation-seed">
    <div className="generation-prompt-heading"><strong>{title}</strong><button onClick={() => void copy()} aria-label={`Copy ${title.toLowerCase()} ${index + 1}`}>{copied ? 'Copied' : 'Copy seed'}</button></div>
    <div className="generation-seed-value" tabIndex={0} aria-label={title}>{seed.value}</div>
    <span className="generation-prompt-source">{seed.source}</span>
    {error && <p role="alert">{error}</p>}
  </div>
}

export default function GenerationPrompt({ probe, image = false }: { probe: Parameters<typeof extractGenerationMetadata>[0]; image?: boolean }) {
  const metadata = useMemo(() => extractGenerationMetadata(probe), [probe])
  return <section className="generation-prompt-section" aria-label="Generation prompt">
    <div className="section-title">GENERATION PROMPT</div>
    {metadata.format ? <>
      <div className="generation-format">Format: <strong>{metadata.format}</strong></div>
      {metadata.prompts.length ? metadata.prompts.map((prompt, index) => <PromptText key={`${prompt.kind}:${prompt.source}:${prompt.text}`} prompt={prompt} index={index} />)
        : <p className="no-data">{metadata.format === 'Unknown' ? 'Metadata found, but no supported prompt could be extracted.' : 'No prompt found in this metadata.'}</p>}
      {metadata.seeds.length ? metadata.seeds.map((seed, index) => <SeedValue key={`${seed.kind}:${seed.source}:${seed.value}`} seed={seed} index={index} image={image} />)
        : <p className="no-data">No seed recorded in this metadata.</p>}
      {metadata.warnings.map(message => <p key={message} className="generation-warning">{message}</p>)}
    </> : <p className="no-data">No generation metadata found.</p>}
  </section>
}
