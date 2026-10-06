import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import ts from 'typescript'

async function loadTypeScript(path) {
  const source = await readFile(new URL(path, import.meta.url), 'utf8')
  const javascript = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
  }).outputText
  return import(`data:text/javascript;base64,${Buffer.from(javascript).toString('base64')}`)
}

const { adjacentFrameTime, frameIndexAt } = await loadTypeScript('../src/frameNavigation.ts')
const { cutPreviewAction } = await loadTypeScript('../src/cutPreview.ts')
const { nextMediaIndex } = await loadTypeScript('../src/mediaNavigation.ts')
const { sortMedia } = await loadTypeScript('../src/mediaSort.ts')
const { extractGenerationMetadata } = await loadTypeScript('../src/generationMetadata.ts')
const expectedPrompt = 'A small red sailboat glides across a calm lake at sunrise. Gentle ripples spread behind it. Mist floats above the water. The camera slowly pans to follow the boat.'
for (const [file, format] of [['comfy', 'ComfyUI'], ['wan2gp', 'WAN2GP']]) {
  const fixture = JSON.parse(await readFile(new URL(`fixtures/${file}-generation.json`, import.meta.url), 'utf8'))
  const extracted = extractGenerationMetadata(fixture)
  assert.equal(extracted.format, format)
  assert.deepEqual(extracted.prompts.map(prompt => [prompt.kind, prompt.text]), [['positive', expectedPrompt]])
  assert.deepEqual(extracted.warnings, [])
}
assert.equal(extractGenerationMetadata({ format: { tags: { encoder: 'ffmpeg' } } }).format, null)
assert.equal(extractGenerationMetadata({ format: { tags: { comment: 'Camera recording' } } }).format, 'Unknown')
assert.equal(extractGenerationMetadata({ format: { tags: { comment: '{broken JSON' } } }).warnings.length, 1)
const wanNoPrompt = { type: 'WanGP v13.141', seed: 42 }
assert.deepEqual(extractGenerationMetadata({ format: { tags: { COMMENT: JSON.stringify(wanNoPrompt) } } }).prompts, [])
assert.equal(extractGenerationMetadata({ format: { tags: { COMMENT: JSON.stringify(wanNoPrompt) } } }).format, 'WAN2GP')
assert.equal(extractGenerationMetadata({ format: { tags: { comment: JSON.stringify({ prompt: 'Unidentified producer' }) } } }).format, 'Unknown')
const graphFixture = {
  out: { class_type: 'VHS_VideoCombine', inputs: { images: ['sample', 0] } },
  sample: { class_type: 'KSampler', inputs: { positive: ['positive', 0], negative: ['negative', 0] } },
  positive: { class_type: 'CLIPTextEncode', inputs: { text: 'Positive\nSecond line' } },
  negative: { class_type: 'ConditioningCombine', inputs: { conditioning_1: ['negativeText', 0], conditioning_2: ['negativeText', 0] } },
  negativeText: { class_type: 'CLIPTextEncode', inputs: { text: 'Blurry' } },
  unused: { class_type: 'CLIPTextEncode', inputs: { text: 'DO NOT EXTRACT' } },
}
const comfyProbe = graph => ({ format: { tags: { prompt: JSON.stringify(graph), workflow: JSON.stringify({ nodes: [{ id: 1, type: 'CLIPTextEncode' }], links: [] }) } } })
const comfyResult = extractGenerationMetadata(comfyProbe(graphFixture))
assert.deepEqual(comfyResult.prompts.map(prompt => [prompt.kind, prompt.text]), [['positive', 'Positive\nSecond line'], ['negative', 'Blurry']])
assert.deepEqual(comfyResult.warnings, [])
const zeroGraph = structuredClone(graphFixture)
zeroGraph.negative.class_type = 'ConditioningZeroOut'
zeroGraph.negative.inputs = { conditioning: ['negativeText', 0] }
assert.deepEqual(extractGenerationMetadata(comfyProbe(zeroGraph)).prompts.map(prompt => prompt.kind), ['positive'])
const cyclicGraph = structuredClone(graphFixture)
cyclicGraph.positive.inputs = { text: ['positive', 0] }
assert.deepEqual(extractGenerationMetadata(comfyProbe(cyclicGraph)).prompts.map(prompt => prompt.text), ['Blurry'])
const workflowOnly = extractGenerationMetadata({ format: { tags: { workflow: JSON.stringify({ nodes: [{ id: 1, type: 'CLIPTextEncode' }], links: [] }) } } })
assert.equal(workflowOnly.format, 'ComfyUI')
assert.equal(workflowOnly.prompts.length, 0)
assert.equal(workflowOnly.warnings.length, 1)
assert.equal(extractGenerationMetadata({ streams: [{ tags: { comment: JSON.stringify({ type: 'Wan2GP v1', prompt: 'Stream prompt', negative_prompt: 'Noise' }) } }] }).prompts.length, 2)
const media = [
  { name: 'clip10.mp4', path: '/10', isDirectory: false, modifiedAt: 300 },
  { name: 'clip2.mp4', path: '/2', isDirectory: false, modifiedAt: 100 },
  { name: 'Folder', path: '/folder', isDirectory: true, modifiedAt: 200 },
  { name: 'clip1.mp4', path: '/1', isDirectory: false, modifiedAt: null },
]
const originalOrder = media.map(entry => entry.path)
assert.deepEqual(sortMedia(media, 'name', 'asc').map(entry => entry.path), ['/folder', '/1', '/2', '/10'])
assert.deepEqual(sortMedia(media, 'name', 'desc').map(entry => entry.path), ['/folder', '/10', '/2', '/1'])
assert.deepEqual(sortMedia(media, 'modified', 'asc').map(entry => entry.path), ['/folder', '/2', '/10', '/1'])
assert.deepEqual(sortMedia(media, 'modified', 'desc').map(entry => entry.path), ['/folder', '/10', '/2', '/1'])
assert.deepEqual(media.map(entry => entry.path), originalOrder)
assert.deepEqual(sortMedia([], 'modified', 'desc'), [])
const sameDate = media.filter(entry => !entry.isDirectory).map(entry => ({ ...entry, modifiedAt: 100 }))
assert.deepEqual(sortMedia(sameDate, 'modified', 'asc').map(entry => entry.path), ['/1', '/2', '/10'])

// Grid navigation remains usable after resizing and in an incomplete final row.
assert.equal(nextMediaIndex(1, 8, 3, 'ArrowDown'), 4)
assert.equal(nextMediaIndex(5, 8, 3, 'ArrowDown'), 7)
assert.equal(nextMediaIndex(7, 8, 3, 'ArrowDown'), 7)
assert.equal(nextMediaIndex(1, 8, 3, 'ArrowUp'), 1)
assert.equal(nextMediaIndex(6, 8, 3, 'ArrowUp'), 3)
assert.equal(nextMediaIndex(2, 8, 3, 'ArrowRight'), 3)
assert.equal(nextMediaIndex(0, 8, 3, 'ArrowLeft'), 0)
assert.equal(nextMediaIndex(3, 8, 1, 'ArrowDown'), 4)
assert.equal(nextMediaIndex(3, 8, 1, 'ArrowUp'), 2)

// These are real variable frame timestamps from a screen recording.
const frames = [0, 0.2424, 0.442967, 0.659433]
assert.equal(adjacentFrameTime(frames, 0, 1), 0.2424)
assert.equal(adjacentFrameTime(frames, 0.208, 1), 0.2424)
assert.equal(adjacentFrameTime(frames, 0.2424, -1), 0)
assert.equal(adjacentFrameTime(frames, 0.659433, 1), 0.659433)
assert.equal(frameIndexAt(frames, 0.208), 0)
assert.equal(frameIndexAt(frames, 0.2424), 1)
assert.equal(adjacentFrameTime([], 0, 1), null)
assert.equal(cutPreviewAction(1.9, 2, 3, 5, true), null)
assert.equal(cutPreviewAction(3.2, 2, 3, 5, true), 'skip') // Time events may jump over a short cut.
assert.equal(cutPreviewAction(3.2, 2, 3, 5, false), null) // The cut was already skipped.
assert.equal(cutPreviewAction(0, 0, 0.5, 5, true), 'skip')
assert.equal(cutPreviewAction(4.2, 4, 5, 5, true), 'stop')

console.log('Frame navigation, cut preview, media sorting, grid navigation, and generation metadata tests passed')
