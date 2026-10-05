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

console.log('Frame navigation, cut preview, media sorting, and grid navigation tests passed')
