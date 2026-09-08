import * as THREE from 'three'
import humanUrl from '../assets/stretch-human.json?url'
import { getStretchPose, STRETCH_JOINT_MAP as JOINT_MAP, type JointName } from './stretchPoses.ts'
import type { StretchExerciseId } from './stretchRoutine.ts'
import { advanceStretchPlayback } from './stretchPlayback.ts'
import { getStretchFraming } from './stretchFraming.ts'

export type StretchScene = {
  ready: Promise<void>
  setExercise: (id: StretchExerciseId) => void
  setReducedMotion: (value: boolean) => void
  setRunning: (value: boolean) => void
  dispose: () => void
}
type HumanAsset = {
  positions: number[]; indices: number[]; skinIndices: number[]; skinWeights: number[]
  bones: { name: string; parent: string | null; position: [number, number, number] }[]
}


// Approximate surface regions, not segmented anatomical muscles.
function regionWeight(id: StretchExerciseId, x: number, y: number, z: number) {
  const spot = (cx: number, cy: number, cz: number, rx: number, ry: number, rz: number) =>
    Math.exp(-2 * (((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 + ((z - cz) / rz) ** 2))
  const neck = spot(0, 3.83, 0.05, 0.34, 0.25, 0.5)
  const shoulders = spot(0.4, 3.54, 0, 0.3, 0.23, 0.6) + spot(-0.4, 3.54, 0, 0.3, 0.23, 0.6)
  switch (id) {
    case 'chin-tuck': return neck
    case 'neck-side-stretch': return neck + shoulders * 0.2
    case 'upper-trapezius': return neck * 0.65 + shoulders * 0.9 + spot(0, 3.45, -0.18, 0.5, 0.38, 0.18)
    case 'shoulder-rolls': return shoulders + spot(0, 3.43, -0.17, 0.48, 0.3, 0.18)
    case 'chest-opener': return spot(0, 3.36, 0.21, 0.6, 0.3, 0.2)
    case 'upper-back-rotation': return spot(0, 3.28, -0.18, 0.5, 0.5, 0.2)
    case 'wrist-forearm': return spot(0.56, 2.57, 0.09, 0.2, 0.43, 0.2) + spot(-0.56, 2.57, 0.09, 0.2, 0.43, 0.2)
    case 'standing-side-bend': return spot(0.31, 2.91, 0, 0.22, 0.43, 0.3) + spot(-0.31, 2.91, 0, 0.22, 0.43, 0.3)
  }
}
function createHuman(asset: HumanAsset) {
  const geometry = new THREE.BufferGeometry()
  geometry.setAttribute('position', new THREE.Float32BufferAttribute(asset.positions, 3))
  geometry.setAttribute('skinIndex', new THREE.Uint16BufferAttribute(asset.skinIndices, 4))
  geometry.setAttribute('skinWeight', new THREE.Float32BufferAttribute(asset.skinWeights, 4))
  geometry.setAttribute('color', new THREE.Float32BufferAttribute(new Float32Array(asset.positions.length), 3))
  geometry.setIndex(asset.indices)
  geometry.computeVertexNormals()
  const material = new THREE.MeshStandardMaterial({ vertexColors: true, roughness: 0.66, metalness: 0.02 })
  // Shade the garment in bind space: crisp hems that follow skinning, without
  // jagged per-vertex color boundaries or a separate clipping-prone shorts mesh.
  material.onBeforeCompile = shader => {
    shader.vertexShader = `varying vec3 guideBindPosition;\n${shader.vertexShader}`
      .replace('#include <begin_vertex>', '#include <begin_vertex>\nguideBindPosition = position;')
    shader.fragmentShader = `varying vec3 guideBindPosition;\n${shader.fragmentShader}`
      .replace('#include <color_fragment>', `#include <color_fragment>
        float garment = step(1.87, guideBindPosition.y) * step(guideBindPosition.y, 2.57) * step(abs(guideBindPosition.x), 0.49);
        diffuseColor.rgb = mix(diffuseColor.rgb, vec3(0.053, 0.067, 0.082), garment);
      `)
  }
  const mesh = new THREE.SkinnedMesh(geometry, material)
  mesh.frustumCulled = false
  mesh.castShadow = true
  mesh.receiveShadow = true
  const bones = asset.bones.map(def => { const bone = new THREE.Bone(); bone.name = def.name; bone.position.fromArray(def.position); return bone })
  const byName = Object.fromEntries(bones.map(bone => [bone.name, bone]))
  asset.bones.forEach((def, i) => (def.parent ? byName[def.parent] : mesh).add(bones[i]))
  mesh.updateMatrixWorld(true)
  const skeleton = new THREE.Skeleton(bones)
  mesh.bind(skeleton)
  const joints = Object.fromEntries(Object.entries(JOINT_MAP).map(([key, name]) => [key, byName[name]])) as Record<JointName, THREE.Bone>
  const headRest = joints.head.position.clone()
  const rootRest = joints.root.position.clone()
  const ivory = new THREE.Color('#dcdeda'), red = new THREE.Color('#c74943')
  const color = new THREE.Color()
  function highlight(id: StretchExerciseId) {
    const colors = geometry.getAttribute('color')
    for (let i = 0; i < asset.positions.length / 3; i++) {
      const x = asset.positions[i * 3], y = asset.positions[i * 3 + 1], z = asset.positions[i * 3 + 2]
      color.copy(ivory).lerp(red, THREE.MathUtils.smoothstep(regionWeight(id, x, y, z), 0.07, 0.7))
      colors.setXYZ(i, color.r, color.g, color.b)
    }
    colors.needsUpdate = true
  }
  return { mesh, skeleton, joints, headRest, rootRest, highlight }
}

export function createStretchScene(container: HTMLElement, initialId: StretchExerciseId): StretchScene {
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'low-power' })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
  renderer.outputColorSpace = THREE.SRGBColorSpace
  renderer.toneMapping = THREE.ACESFilmicToneMapping
  renderer.toneMappingExposure = 1.12
  renderer.shadowMap.enabled = true
  renderer.shadowMap.type = THREE.PCFShadowMap
  renderer.domElement.className = 'stretch-canvas'
  renderer.domElement.setAttribute('aria-hidden', 'true')
  container.dataset.state = 'loading'
  container.append(renderer.domElement)
  const scene = new THREE.Scene()
  const camera = new THREE.OrthographicCamera(-2, 2, 2.5, -2.5, 0.1, 30)
  scene.add(new THREE.HemisphereLight(0xffffff, 0x9c9da2, 2))
  const key = new THREE.DirectionalLight(0xffffff, 3.1)
  key.position.set(-3, 7, 5)
  key.castShadow = true
  key.shadow.mapSize.set(1024, 1024)
  Object.assign(key.shadow.camera, { left: -3, right: 3, top: 5, bottom: -2 })
  key.shadow.bias = -0.0004
  key.shadow.normalBias = 0.025
  scene.add(key)
  const rim = new THREE.DirectionalLight(0xe5ebf1, 2)
  rim.position.set(3, 5, -4)
  scene.add(rim)
  const ground = new THREE.Mesh(new THREE.PlaneGeometry(20, 20), new THREE.ShadowMaterial({ color: 0x404750, opacity: 0.14 }))
  ground.rotation.x = -Math.PI / 2
  ground.position.y = -0.015
  ground.receiveShadow = true
  scene.add(ground)
  let rig: ReturnType<typeof createHuman> | undefined
  let exerciseId = initialId
  let elapsed = 0
  let previousFrame = performance.now()
  let running = true
  let reducedMotion = false
  let disposed = false
  let frame = 0
  const controller = new AbortController()
  const schedule = () => { if (!disposed && !frame) frame = requestAnimationFrame(render) }
  const resize = () => {
    renderer.setSize(Math.max(1, container.clientWidth), Math.max(1, container.clientHeight), false)
    schedule()
  }
  const resizeObserver = new ResizeObserver(resize)
  resizeObserver.observe(container)
  const ready = fetch(humanUrl, { signal: controller.signal }).then(response => {
    if (!response.ok) throw new Error('Human asset unavailable')
    return response.json() as Promise<HumanAsset>
  }).then(asset => {
    if (disposed) return
    rig = createHuman(asset)
    rig.highlight(exerciseId)
    scene.add(rig.mesh)
    container.dataset.state = 'ready'
    previousFrame = performance.now()
    schedule()
  }).catch(error => {
    if (disposed) return
    container.dataset.state = 'error'
    throw error
  })
  function render(now: number) {
    frame = 0
    if (disposed) return
    const delta = now - previousFrame
    previousFrame = now
    const moving = running && !reducedMotion && !document.hidden
    elapsed = advanceStretchPlayback(elapsed, delta, moving && !!rig)
    const pose = getStretchPose(exerciseId, reducedMotion ? 0.25 : (elapsed / 16000) % 1, reducedMotion)
    if (rig) {
      const blend = reducedMotion || !running ? 1 : 1 - Math.exp(-Math.min(delta, 50) / 140)
      for (const name of Object.keys(JOINT_MAP) as JointName[]) {
        const rotation = pose.joints[name], joint = rig.joints[name]
        joint.rotation.x += (rotation[0] - joint.rotation.x) * blend
        joint.rotation.y += (rotation[1] - joint.rotation.y) * blend
        joint.rotation.z += (rotation[2] - joint.rotation.z) * blend
      }
      rig.joints.head.position.copy(rig.headRest)
      rig.joints.head.position.z += pose.headRetraction * 0.55
      rig.joints.root.position.copy(rig.rootRest).add(new THREE.Vector3(...pose.rootPosition))
    }
    const aspect = Math.max(1, container.clientWidth) / Math.max(1, container.clientHeight)
    const { yaw, viewHeight, targetY } = getStretchFraming(exerciseId, aspect)
    camera.left = -viewHeight * aspect / 2
    camera.right = viewHeight * aspect / 2
    camera.top = viewHeight / 2
    camera.bottom = -viewHeight / 2
    camera.updateProjectionMatrix()
    camera.position.set(Math.sin(yaw) * 9, targetY + 0.12, Math.cos(yaw) * 9)
    camera.lookAt(0, targetY, 0)
    if (!document.hidden) renderer.render(scene, camera)
    if (moving && rig) schedule()
  }
  const onVisibilityChange = () => { previousFrame = performance.now(); schedule() }
  document.addEventListener('visibilitychange', onVisibilityChange)
  resize()
  return {
    ready,
    setExercise(id) {
      if (id === exerciseId) return
      exerciseId = id
      elapsed = 0
      rig?.highlight(id)
      schedule()
    },
    setReducedMotion(value) { reducedMotion = value; schedule() },
    setRunning(value) { running = value; previousFrame = performance.now(); schedule() },
    dispose() {
      if (disposed) return
      disposed = true
      controller.abort()
      cancelAnimationFrame(frame)
      resizeObserver.disconnect()
      document.removeEventListener('visibilitychange', onVisibilityChange)
      rig?.skeleton.dispose()
      scene.traverse(object => {
        if (!(object instanceof THREE.Mesh)) return
        object.geometry.dispose()
        const materials = Array.isArray(object.material) ? object.material : [object.material]
        materials.forEach(material => material.dispose())
      })
      key.shadow.dispose()
      renderer.dispose()
      renderer.forceContextLoss()
      renderer.domElement.remove()
    },
  }
}
