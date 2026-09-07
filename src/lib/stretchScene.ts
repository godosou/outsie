import * as THREE from 'three'
import { getStretchPose, type JointName } from './stretchPoses.ts'
import type { StretchExerciseId } from './stretchRoutine.ts'

export type StretchScene = {
  setExercise: (id: StretchExerciseId) => void
  setReducedMotion: (value: boolean) => void
  dispose: () => void
}

type Rig = {
  avatar: THREE.Group
  joints: Record<JointName, THREE.Group>
  chestBody: THREE.Mesh
}

const COLORS = {
  skin: 0xdcae91,
  skinShadow: 0xc89174,
  top: 0x758f65,
  topLight: 0x91a981,
  shorts: 0x526a50,
  leggings: 0xc7b6a1,
  shoes: 0xf4efe4,
  hair: 0x4b403a,
  eyes: 0x302f2b,
}

function clay(color: number, roughness = 0.84) {
  return new THREE.MeshStandardMaterial({ color, roughness, metalness: 0 })
}

function capsule(radius: number, length: number, material: THREE.Material, segments = 18) {
  const mesh = new THREE.Mesh(new THREE.CapsuleGeometry(radius, length, 8, segments), material)
  mesh.castShadow = true
  mesh.receiveShadow = true
  return mesh
}

function sphere(radius: number, material: THREE.Material, width = 24, height = 18) {
  const mesh = new THREE.Mesh(new THREE.SphereGeometry(radius, width, height), material)
  mesh.castShadow = true
  mesh.receiveShadow = true
  return mesh
}

function addArm(
  chest: THREE.Group,
  side: 'left' | 'right',
  materials: { skin: THREE.Material; top: THREE.Material },
  joints: Partial<Record<JointName, THREE.Group>>,
) {
  const direction = side === 'left' ? -1 : 1
  const shoulderName = `${side}Shoulder` as JointName
  const elbowName = `${side}Elbow` as JointName
  const wristName = `${side}Wrist` as JointName
  const shoulder = new THREE.Group()
  shoulder.position.set(direction * 0.58, 0.43, 0)
  chest.add(shoulder)
  joints[shoulderName] = shoulder

  const sleeve = capsule(0.175, 0.24, materials.top)
  sleeve.position.y = -0.25
  shoulder.add(sleeve)
  const upperArm = capsule(0.135, 0.29, materials.skin)
  upperArm.position.y = -0.61
  shoulder.add(upperArm)

  const elbow = new THREE.Group()
  elbow.position.y = -0.86
  shoulder.add(elbow)
  joints[elbowName] = elbow
  const elbowJoint = sphere(0.14, materials.skin)
  elbow.add(elbowJoint)
  const forearm = capsule(0.125, 0.42, materials.skin)
  forearm.position.y = -0.34
  elbow.add(forearm)

  const wrist = new THREE.Group()
  wrist.position.y = -0.7
  elbow.add(wrist)
  joints[wristName] = wrist
  const hand = capsule(0.11, 0.15, materials.skin, 14)
  hand.position.y = -0.16
  hand.scale.set(0.85, 1, 0.55)
  wrist.add(hand)
}

function addLeg(
  avatar: THREE.Group,
  side: 'left' | 'right',
  materials: { leggings: THREE.Material; skin: THREE.Material; shoes: THREE.Material },
  joints: Partial<Record<JointName, THREE.Group>>,
) {
  const direction = side === 'left' ? -1 : 1
  const hipName = `${side}Hip` as JointName
  const kneeName = `${side}Knee` as JointName
  const hip = new THREE.Group()
  hip.position.set(direction * 0.245, -0.16, 0)
  avatar.add(hip)
  joints[hipName] = hip

  const thigh = capsule(0.205, 0.58, materials.leggings)
  thigh.position.y = -0.48
  hip.add(thigh)
  const knee = new THREE.Group()
  knee.position.y = -0.96
  hip.add(knee)
  joints[kneeName] = knee
  knee.add(sphere(0.19, materials.leggings))
  const shin = capsule(0.17, 0.58, materials.skin)
  shin.position.y = -0.48
  knee.add(shin)
  const shoe = capsule(0.18, 0.25, materials.shoes, 14)
  shoe.rotation.x = Math.PI / 2
  shoe.scale.set(1, 1.25, 0.72)
  shoe.position.set(0, -0.91, 0.12)
  knee.add(shoe)
}

function createRig(): Rig {
  const materials = {
    skin: clay(COLORS.skin, 0.9),
    skinShadow: clay(COLORS.skinShadow, 0.9),
    top: clay(COLORS.top),
    topLight: clay(COLORS.topLight),
    shorts: clay(COLORS.shorts),
    leggings: clay(COLORS.leggings),
    shoes: clay(COLORS.shoes),
    hair: clay(COLORS.hair, 0.92),
    eyes: clay(COLORS.eyes, 0.72),
  }
  const avatar = new THREE.Group()
  const partialJoints: Partial<Record<JointName, THREE.Group>> = { root: avatar }

  const pelvis = capsule(0.38, 0.24, materials.shorts)
  pelvis.scale.set(1.08, 0.9, 0.82)
  avatar.add(pelvis)

  const torso = new THREE.Group()
  torso.position.y = 0.2
  avatar.add(torso)
  partialJoints.torso = torso
  const waist = capsule(0.39, 0.42, materials.top)
  waist.position.y = 0.38
  waist.scale.set(0.92, 1, 0.74)
  torso.add(waist)

  const chest = new THREE.Group()
  chest.position.y = 0.76
  torso.add(chest)
  partialJoints.chest = chest
  const chestBody = capsule(0.47, 0.42, materials.topLight)
  chestBody.position.y = 0.2
  chestBody.scale.set(1, 1, 0.75)
  chest.add(chestBody)

  const neck = new THREE.Group()
  neck.position.y = 0.76
  chest.add(neck)
  partialJoints.neck = neck
  const neckMesh = capsule(0.115, 0.17, materials.skin, 14)
  neckMesh.position.y = 0.08
  neck.add(neckMesh)

  const head = new THREE.Group()
  head.position.y = 0.31
  neck.add(head)
  partialJoints.head = head
  const face = sphere(0.34, materials.skin)
  face.scale.set(0.9, 1.06, 0.91)
  head.add(face)
  const hair = sphere(0.355, materials.hair)
  hair.scale.set(0.94, 0.78, 0.95)
  hair.position.set(0, 0.13, -0.06)
  head.add(hair)
  const bun = sphere(0.16, materials.hair, 18, 14)
  bun.position.set(0.23, 0.29, -0.09)
  head.add(bun)
  for (const x of [-0.1, 0.1]) {
    const eye = sphere(0.025, materials.eyes, 12, 10)
    eye.position.set(x, 0.035, 0.303)
    eye.scale.y = 1.15
    head.add(eye)
  }
  const nose = sphere(0.035, materials.skinShadow, 12, 10)
  nose.scale.set(0.72, 1, 0.72)
  nose.position.set(0, -0.025, 0.337)
  head.add(nose)

  addArm(chest, 'left', materials, partialJoints)
  addArm(chest, 'right', materials, partialJoints)
  addLeg(avatar, 'left', materials, partialJoints)
  addLeg(avatar, 'right', materials, partialJoints)

  const joints = partialJoints as Record<JointName, THREE.Group>
  return { avatar, joints, chestBody }
}

function disposeObject(root: THREE.Object3D) {
  const materials = new Set<THREE.Material>()
  const geometries = new Set<THREE.BufferGeometry>()
  root.traverse(object => {
    if (!(object instanceof THREE.Mesh)) return
    geometries.add(object.geometry)
    const meshMaterials = Array.isArray(object.material) ? object.material : [object.material]
    meshMaterials.forEach(material => materials.add(material))
  })
  geometries.forEach(geometry => geometry.dispose())
  materials.forEach(material => material.dispose())
}

export function createStretchScene(container: HTMLElement, initialId: StretchExerciseId): StretchScene {
  const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, powerPreference: 'high-performance' })
  renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2))
  renderer.outputColorSpace = THREE.SRGBColorSpace
  renderer.toneMapping = THREE.ACESFilmicToneMapping
  renderer.toneMappingExposure = 1.08
  renderer.shadowMap.enabled = true
  renderer.shadowMap.type = THREE.PCFSoftShadowMap
  renderer.domElement.className = 'stretch-canvas'
  renderer.domElement.setAttribute('aria-hidden', 'true')
  container.append(renderer.domElement)

  const scene = new THREE.Scene()
  const camera = new THREE.PerspectiveCamera(29, 1, 0.1, 30)
  const rig = createRig()
  rig.avatar.position.y = -0.08
  scene.add(rig.avatar)

  const hemisphere = new THREE.HemisphereLight(0xfff9ec, 0x829077, 2.8)
  scene.add(hemisphere)
  const key = new THREE.DirectionalLight(0xfff2db, 4.3)
  key.position.set(-3.5, 6, 5)
  key.castShadow = true
  key.shadow.mapSize.set(1024, 1024)
  key.shadow.camera.left = -4
  key.shadow.camera.right = 4
  key.shadow.camera.top = 5
  key.shadow.camera.bottom = -4
  key.shadow.bias = -0.0004
  scene.add(key)
  const rim = new THREE.DirectionalLight(0xcde1bb, 2.1)
  rim.position.set(4, 2, -3)
  scene.add(rim)

  const groundMaterial = new THREE.ShadowMaterial({ color: 0x31432c, opacity: 0.17 })
  const ground = new THREE.Mesh(new THREE.CircleGeometry(2.35, 64), groundMaterial)
  ground.rotation.x = -Math.PI / 2
  ground.position.y = -2.08
  ground.receiveShadow = true
  scene.add(ground)

  const haloMaterial = new THREE.MeshBasicMaterial({ color: 0xdbe7cf, transparent: true, opacity: 0.46, side: THREE.DoubleSide })
  const halo = new THREE.Mesh(new THREE.TorusGeometry(2.02, 0.018, 8, 96), haloMaterial)
  halo.position.set(0, 0.15, -0.75)
  scene.add(halo)

  let exerciseId = initialId
  let exerciseStartedAt = performance.now()
  let reducedMotion = false
  let disposed = false
  let frame = 0

  const resize = () => {
    const width = Math.max(1, container.clientWidth)
    const height = Math.max(1, container.clientHeight)
    renderer.setSize(width, height, false)
    camera.aspect = width / height
    camera.updateProjectionMatrix()
  }
  const resizeObserver = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(resize)
  resizeObserver?.observe(container)
  window.addEventListener('resize', resize)
  resize()

  const render = (now: number) => {
    if (disposed) return
    const phase = reducedMotion ? 0.25 : ((now - exerciseStartedAt) / 5200) % 1
    const pose = getStretchPose(exerciseId, phase, reducedMotion)
    for (const [name, rotation] of Object.entries(pose.joints) as [JointName, readonly [number, number, number]][]) {
      rig.joints[name].rotation.set(rotation[0], rotation[1], rotation[2])
    }
    rig.avatar.position.set(pose.rootPosition[0], pose.rootPosition[1] - 0.08, pose.rootPosition[2])
    const breath = reducedMotion ? 0 : Math.sin(now / 950) * 0.012
    rig.chestBody.scale.set(1 + breath * 0.4, 1 + breath, 0.75 + breath * 0.3)
    const cameraYaw = pose.cameraYaw
    camera.position.set(Math.sin(cameraYaw) * 9.8, 0.12, Math.cos(cameraYaw) * 9.8)
    camera.lookAt(0, 0.1, 0)
    halo.rotation.z = reducedMotion ? 0.08 : 0.08 + Math.sin(now / 2600) * 0.025
    renderer.render(scene, camera)
    frame = requestAnimationFrame(render)
  }
  frame = requestAnimationFrame(render)

  return {
    setExercise(id) {
      if (id === exerciseId) return
      exerciseId = id
      exerciseStartedAt = performance.now()
    },
    setReducedMotion(value) {
      reducedMotion = value
    },
    dispose() {
      if (disposed) return
      disposed = true
      cancelAnimationFrame(frame)
      resizeObserver?.disconnect()
      window.removeEventListener('resize', resize)
      disposeObject(scene)
      renderer.dispose()
      renderer.forceContextLoss()
      renderer.domElement.remove()
    },
  }
}
