import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import ts from 'typescript'

const source = await readFile(new URL('../src/frameNavigation.ts', import.meta.url), 'utf8')
const javascript = ts.transpileModule(source, {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText
const { adjacentFrameTime, frameIndexAt } = await import(
  `data:text/javascript;base64,${Buffer.from(javascript).toString('base64')}`
)

// These are real variable frame timestamps from a screen recording.
const frames = [0, 0.2424, 0.442967, 0.659433]
assert.equal(adjacentFrameTime(frames, 0, 1), 0.2424)
assert.equal(adjacentFrameTime(frames, 0.208, 1), 0.2424)
assert.equal(adjacentFrameTime(frames, 0.2424, -1), 0)
assert.equal(adjacentFrameTime(frames, 0.659433, 1), 0.659433)
assert.equal(frameIndexAt(frames, 0.208), 0)
assert.equal(frameIndexAt(frames, 0.2424), 1)
assert.equal(adjacentFrameTime([], 0, 1), null)

console.log('Frame navigation tests passed')
