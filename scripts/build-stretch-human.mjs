// Converts CC0 MakeHuman core data; no MakeHuman application code is used.
// Usage: node scripts/build-stretch-human.mjs base.obj default.mhskel default_weights.mhw
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import * as THREE from 'three'

const [objPath, skeletonPath, weightsPath, ...targetPaths] = process.argv.slice(2)
if (!weightsPath) throw new Error('Supply the three official MakeHuman source assets; see assets attribution.')
const sourceRig = JSON.parse(readFileSync(skeletonPath, 'utf8'))
const sourceWeights = JSON.parse(readFileSync(weightsPath, 'utf8')).weights
const vertices = [], faces = []
let group = ''
for (const line of readFileSync(objPath, 'utf8').split('\n')) {
  const [kind, ...values] = line.trim().split(/\s+/)
  if (kind === 'v') vertices.push(values.map(Number))
  if (kind === 'g') group = values[0]
  if (kind === 'f' && group === 'body') {
    const face = values.map(value => Number(value.split('/')[0]) - 1)
    for (let i = 1; i < face.length - 1; i++) faces.push(face[0], face[i], face[i + 1])
  }
}
for (const path of targetPaths) {
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    if (!/^\d+\s/.test(line)) continue
    const [index, ...delta] = line.trim().split(/\s+/).map(Number)
    for (let axis = 0; axis < 3; axis++) vertices[index][axis] += delta[axis]
  }
}
const used = [...new Set(faces)].sort((a, b) => a - b)
const remap = new Map(used.map((old, index) => [old, index]))
const minY = Math.min(...used.map(i => vertices[i][1]))
const maxY = Math.max(...used.map(i => vertices[i][1]))
const scale = 4.4 / (maxY - minY)
const point = v => new THREE.Vector3(v[0] * scale, (v[1] - minY) * scale, v[2] * scale)
const jointPoint = name => point(sourceRig.joints[name].reduce((sum, i, _, all) => sum.map((v, k) => v + vertices[i][k] / all.length), [0, 0, 0]))
const names = Object.keys(sourceRig.bones)
const bones = names.map(name => { const b = new THREE.Bone(); b.name = name; return b })
const boneByName = Object.fromEntries(bones.map(b => [b.name, b]))
for (const bone of bones) {
  const definition = sourceRig.bones[bone.name]
  bone.position.copy(jointPoint(definition.head))
  if (definition.parent) {
    bone.position.sub(jointPoint(sourceRig.bones[definition.parent].head))
    boneByName[definition.parent].add(bone)
  }
}
const influences = vertices.map(() => [])
for (const [name, weights] of Object.entries(sourceWeights)) {
  const index = names.indexOf(name)
  if (index < 0) throw new Error(`Unknown weight bone ${name}`)
  for (const [vertex, weight] of weights) if (weight > 0) influences[vertex].push([index, weight])
}
const skinIndices = [], skinWeights = []
for (const old of used) {
  const weights = influences[old].sort((a, b) => b[1] - a[1]).slice(0, 4)
  if (!weights.length) throw new Error(`Unweighted body vertex ${old}`)
  const total = weights.reduce((sum, entry) => sum + entry[1], 0)
  for (let i = 0; i < 4; i++) {
    skinIndices.push(weights[i]?.[0] ?? 0)
    skinWeights.push((weights[i]?.[1] ?? 0) / total)
  }
}
const geometry = new THREE.BufferGeometry()
geometry.setAttribute('position', new THREE.Float32BufferAttribute(used.flatMap(i => point(vertices[i]).toArray()), 3))
geometry.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(skinIndices, 4))
geometry.setAttribute('skinWeight', new THREE.Float32BufferAttribute(skinWeights, 4))
const mesh = new THREE.SkinnedMesh(geometry, new THREE.MeshBasicMaterial())
mesh.add(boneByName.root)
mesh.updateMatrixWorld(true)
const skeleton = new THREE.Skeleton(bones)
mesh.bind(skeleton)

// Bake a relaxed standing bind pose instead of animating from the source A-pose.
// Keep all resulting rest axes world-aligned so the teaching pose API is predictable.
for (const side of ['L', 'R']) {
  for (const [name, child] of [[`upperarm01.${side}`, `lowerarm01.${side}`], [`lowerarm01.${side}`, `wrist.${side}`]]) {
    const bone = boneByName[name]
    mesh.updateMatrixWorld(true)
    const direction = boneByName[child].getWorldPosition(new THREE.Vector3()).sub(bone.getWorldPosition(new THREE.Vector3())).normalize()
    const desired = new THREE.Vector3(side === 'L' ? 0.11 : -0.11, -1, 0.04).normalize()
    const worldRotation = new THREE.Quaternion().setFromUnitVectors(direction, desired).multiply(bone.getWorldQuaternion(new THREE.Quaternion()))
    bone.quaternion.copy(bone.parent.getWorldQuaternion(new THREE.Quaternion()).invert().multiply(worldRotation))
  }
}
mesh.updateMatrixWorld(true)
skeleton.update()
const position = geometry.getAttribute('position')
const positions = []
for (let i = 0; i < position.count; i++) positions.push(...mesh.applyBoneTransform(i, new THREE.Vector3().fromBufferAttribute(position, i)).toArray())
const worldPositions = Object.fromEntries(bones.map(b => [b.name, b.getWorldPosition(new THREE.Vector3())]))
const round = n => Math.round(n * 1e6) / 1e6
const asset = {
  license: 'CC0-1.0',
  source: 'MakeHuman Community core base mesh and default rig',
  positions: positions.map(round),
  indices: faces.map(i => remap.get(i)),
  skinIndices,
  skinWeights: skinWeights.map(round),
  bones: bones.map(b => ({
    name: b.name,
    parent: sourceRig.bones[b.name].parent,
    position: worldPositions[b.name].clone().sub(sourceRig.bones[b.name].parent ? worldPositions[sourceRig.bones[b.name].parent] : new THREE.Vector3()).toArray().map(round),
  })),
}
mkdirSync(new URL('../src/assets/', import.meta.url), { recursive: true })
writeFileSync(new URL('../src/assets/stretch-human.json', import.meta.url), JSON.stringify(asset))
console.log(`Generated ${used.length} body vertices, ${faces.length / 3} triangles, ${bones.length} bones`)
