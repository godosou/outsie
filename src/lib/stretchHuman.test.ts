import test from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, readFileSync } from 'node:fs'

const assetPath = new URL('../assets/stretch-human.json', import.meta.url)
test('anatomical guide bundles a proportionate, weighted human surface for offline use', () => {
  assert.ok(existsSync(assetPath), 'local human asset must exist')
  const asset = JSON.parse(readFileSync(assetPath, 'utf8'))
  assert.equal(asset.license, 'CC0-1.0')
  assert.ok(asset.positions.length > 30000, 'continuous body, not primitive capsules')
  assert.equal(asset.skinIndices.length, asset.positions.length / 3 * 4)
  assert.equal(asset.skinWeights.length, asset.skinIndices.length)
  for (let i = 0; i < asset.skinWeights.length; i += 4) {
    assert.ok(Math.abs(asset.skinWeights.slice(i, i + 4).reduce((a: number, b: number) => a + b, 0) - 1) < 0.001)
  }
  for (const i of asset.indices) assert.ok(i >= 0 && i < asset.positions.length / 3)
  for (const i of asset.skinIndices) assert.ok(i >= 0 && i < asset.bones.length)
  for (const bone of asset.bones) {
    assert.ok(bone.position.every(Number.isFinite))
    assert.ok(bone.parent === null || asset.bones.some((parent: { name: string }) => parent.name === bone.parent))
  }
  for (const name of ['head', 'neck01', 'spine01', 'upperarm01.L', 'lowerarm01.R', 'wrist.L']) {
    assert.ok(asset.bones.some((bone: { name: string }) => bone.name === name), `missing ${name}`)
  }
})
