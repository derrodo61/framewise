export type ExtractedPrompt = { kind: 'positive' | 'negative'; text: string; source: string }
export type GenerationMetadata = {
  format: 'ComfyUI' | 'WAN2GP' | 'Unknown' | null
  prompts: ExtractedPrompt[]
  warnings: string[]
}
type RecordValue = Record<string, unknown>
type GraphNode = { class_type: string; inputs: RecordValue; _meta?: { title?: string } }
type Graph = Record<string, GraphNode>
type ProbeMetadata = { format?: { tags?: Record<string, string> }; streams?: Array<{ tags?: Record<string, string> }> }

function record(value: unknown): value is RecordValue {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}
function decode(value: unknown): unknown {
  for (let depth = 0; depth < 4 && typeof value === 'string'; depth++) {
    try { value = JSON.parse(value) } catch { break }
  }
  return value
}
function graph(value: unknown): Graph | null {
  value = decode(value)
  if (!record(value)) return null
  const nodes = Object.values(value)
  return nodes.length > 0 && nodes.every(node => record(node) && typeof node.class_type === 'string' && record(node.inputs)) ? value as Graph : null
}
function workflow(value: unknown): boolean {
  value = decode(value)
  return record(value) && Array.isArray(value.links) && Array.isArray(value.nodes)
    && value.nodes.some(node => record(node) && typeof node.type === 'string' && (typeof node.id === 'number' || typeof node.id === 'string'))
}
function link(value: unknown, nodes: Graph): string | null {
  if (!Array.isArray(value) || value.length !== 2 || !Number.isInteger(value[1])) return null
  const id = String(value[0])
  return Object.hasOwn(nodes, id) ? id : null
}
function addPrompt(result: GenerationMetadata, kind: ExtractedPrompt['kind'], value: unknown, source: string) {
  if (typeof value !== 'string' || !value.trim()) return
  if (!result.prompts.some(prompt => prompt.kind === kind && prompt.text === value)) result.prompts.push({ kind, text: value, source })
}

function extractComfy(nodes: Graph, result: GenerationMetadata) {
  const outputs = Object.keys(nodes).filter(id => /^(VHS_VideoCombine|SaveVideo|SaveAnimatedWEBP|SaveAnimatedPNG)$/i.test(nodes[id].class_type))
  if (!outputs.length) {
    result.warnings.push('No supported video output node was found. The execution graph is available in raw metadata.')
    return
  }
  const reachable = new Set<string>()
  const pending = [...outputs]
  while (pending.length) {
    const id = pending.pop()!
    if (reachable.has(id)) continue
    reachable.add(id)
    for (const input of Object.values(nodes[id].inputs)) {
      const upstream = link(input, nodes)
      if (upstream) pending.push(upstream)
    }
  }

  const seen = new Set<string>()
  function textFrom(value: unknown, kind: ExtractedPrompt['kind'], source: string, depth = 0) {
    const id = link(value, nodes)
    if (!id) { addPrompt(result, kind, value, source); return }
    const key = `${id}:${kind}`
    if (seen.has(key) || depth > 64) return
    seen.add(key)
    const node = nodes[id]
    // Zeroed conditioning does not contribute a text prompt.
    if (node.class_type === 'ConditioningZeroOut') return
    const label = `${node._meta?.title || node.class_type} (node ${id})`
    const fields = Object.entries(node.inputs).filter(([name]) => /^(text(?:_[gl])?|prompt|value|string|positive|positive_prompt|negative|negative_prompt|conditioning(?:_\d+)?)$/i.test(name))
    if (!fields.length) {
      result.warnings.push(`Cannot resolve prompt through ${label}.`)
      return
    }
    if (!/^(Text|PrimitiveNode|PrimitiveString(?:Multiline)?|CLIPTextEncode(?:SDXL|SDXLRefiner)?|ConditioningCombine|ConditioningConcat|ConditioningAverage|MiniMaxH3ImageToVideo)$/i.test(node.class_type)) {
      result.warnings.push(`${label} may transform the text. The displayed text is its input, not a reconstructed output.`)
    }
    for (const [name, input] of fields) textFrom(input, /negative/i.test(name) ? 'negative' : kind, label, depth + 1)
  }

  for (const id of reachable) {
    const node = nodes[id]
    // Only follow semantic prompt/conditioning inputs in the active output branch.
    for (const [name, input] of Object.entries(node.inputs)) {
      if (/^(prompt|positive|positive_prompt|negative|negative_prompt)$/i.test(name) || (name === 'conditioning' && /Guider$/i.test(node.class_type))) {
        textFrom(input, /negative/i.test(name) ? 'negative' : 'positive', `${node._meta?.title || node.class_type} (node ${id})`)
      }
    }
  }
}

export function extractGenerationMetadata(probe: ProbeMetadata): GenerationMetadata {
  const result: GenerationMetadata = { format: null, prompts: [], warnings: [] }
  const detected = new Set<'ComfyUI' | 'WAN2GP'>()
  let hasExecution = false
  const tags = [probe.format?.tags, ...(probe.streams ?? []).map(stream => stream.tags)].filter(Boolean)
  let candidate = false
  for (const scope of tags) {
    for (const [originalKey, raw] of Object.entries(scope!)) {
      const key = originalKey.toLowerCase()
      if (!['comment', 'description', 'prompt', 'workflow', 'parameters'].includes(key)) continue
      candidate = true
      const payload = decode(raw)
      const directGraph = key === 'prompt' ? graph(payload) : null
      const nestedGraph = record(payload) ? graph(payload.prompt) : null
      if (directGraph || nestedGraph || (key === 'workflow' && workflow(payload)) || (record(payload) && workflow(payload.workflow))) {
        detected.add('ComfyUI')
        result.format = 'ComfyUI'
        const execution = directGraph ?? nestedGraph
        if (execution) { hasExecution = true; extractComfy(execution, result) }
        continue
      }
      if (record(payload) && (typeof payload.type === 'string' && /^Wan(?:2)?GP\b/i.test(payload.type))) {
        detected.add('WAN2GP')
        result.format = 'WAN2GP'
        addPrompt(result, 'positive', payload.prompt, `${originalKey} → prompt`)
        addPrompt(result, 'negative', payload.negative_prompt, `${originalKey} → negative_prompt`)
        if (typeof payload.prompt !== 'string' && payload.prompt !== undefined) result.warnings.push('The WAN2GP prompt field has an unsupported structure.')
        continue
      }
      if (record(payload)) {
        // A direct text field is useful even if the generator cannot be identified.
        addPrompt(result, 'positive', payload.prompt, `${originalKey} → prompt (unknown format)`)
        addPrompt(result, 'negative', payload.negative_prompt, `${originalKey} → negative_prompt (unknown format)`)
      } else if (key === 'prompt' && typeof payload === 'string' && !/^\s*[[{]/.test(payload)) {
        addPrompt(result, 'positive', payload, `${originalKey} (unknown format)`)
      }
      if (typeof payload === 'string' && /^\s*[[{]/.test(payload)) result.warnings.push(`The ${originalKey} tag contains invalid or unsupported JSON.`)
    }
  }
  if (!result.format && candidate) result.format = 'Unknown'
  if (detected.has('ComfyUI') && !hasExecution) result.warnings.push('ComfyUI workflow found, but no usable execution graph. Prompt extraction requires the executed node connections.')
  if (detected.size > 1) { result.format = 'Unknown'; result.warnings.push('More than one generation metadata format is present. Prompts are labelled with their individual sources.') }
  result.warnings = [...new Set(result.warnings)]
  return result
}
