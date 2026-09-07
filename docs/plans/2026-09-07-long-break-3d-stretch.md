# Long-Break 3D Stretch Training Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add an offline, soft-clay 3D stretch trainer that automatically cycles through eight guided movements during long breaks in both the React modal and Tauri full-screen cover.

**Architecture:** A pure TypeScript routine module owns exercise metadata and derives the current exercise from the authoritative break countdown. A shared Three.js scene renders a procedural articulated character; thin React and standalone Tauri views reuse the same routine and scene while retaining the existing timer and strict-break behavior.

**Tech Stack:** React 19, TypeScript, Three.js, Vite multi-page build, Node test runner, Tauri 2.

---

### Task 1: Stretch routine state and exercise catalog

**Files:**
- Create: `src/lib/stretchRoutine.ts`
- Create: `src/lib/stretchRoutine.test.ts`
- Modify: `package.json`

**Step 1: Write the failing tests**

Cover these observable behaviors with `node:test` and `node:assert/strict`:

```ts
test('the routine exposes eight unique and safe office stretches', () => {
  assert.equal(STRETCH_EXERCISES.length, 8)
  assert.equal(new Set(STRETCH_EXERCISES.map(item => item.id)).size, 8)
  assert.ok(STRETCH_EXERCISES.filter(item => item.focus === '肩颈与上背').length >= 5)
  assert.ok(STRETCH_EXERCISES.every(item => item.cue && item.safety))
})

test('the routine advances every 30 seconds and loops', () => {
  assert.equal(getStretchStep(300, 300).index, 0)
  assert.equal(getStretchStep(270, 300).index, 1)
  assert.equal(getStretchStep(60, 300).index, 0)
})

test('a manual offset wraps without changing the break clock', () => {
  assert.equal(getStretchStep(270, 300, -2).index, 7)
  assert.equal(getStretchStep(270, 300, 9).index, 2)
})
```

**Step 2: Run tests and verify RED**

Run: `node --import tsx --test src/lib/stretchRoutine.test.ts`

Expected: FAIL because `stretchRoutine.ts` does not exist.

**Step 3: Implement the minimum pure routine module**

Define `STRETCH_STEP_SECONDS = 30`, the eight approved exercises, and:

```ts
export function getStretchStep(remaining: number, duration: number, offset = 0) {
  const elapsed = Math.max(0, duration - remaining)
  const automatic = Math.floor(elapsed / STRETCH_STEP_SECONDS)
  const index = modulo(automatic + offset, STRETCH_EXERCISES.length)
  return {
    index,
    exercise: STRETCH_EXERCISES[index],
    progress: (elapsed % STRETCH_STEP_SECONDS) / STRETCH_STEP_SECONDS,
    remaining: STRETCH_STEP_SECONDS - (elapsed % STRETCH_STEP_SECONDS),
  }
}
```

**Step 4: Run tests and verify GREEN**

Run: `npm test`

Expected: all timer and stretch routine tests pass.

**Step 5: Commit**

```bash
git add package.json src/lib/stretchRoutine.ts src/lib/stretchRoutine.test.ts
git commit -m "feat: define long-break stretch routine"
```

### Task 2: Procedural 3D character and pose animation

**Files:**
- Create: `src/lib/stretchPoses.ts`
- Create: `src/lib/stretchPoses.test.ts`
- Create: `src/lib/stretchScene.ts`
- Modify: `package.json`
- Modify: `package-lock.json`

**Step 1: Write failing pose tests**

Test that every exercise ID resolves to a complete, finite pose and that interpolation produces stable values:

```ts
test('every exercise produces finite joint rotations', () => {
  for (const exercise of STRETCH_EXERCISES) {
    const pose = getStretchPose(exercise.id, 0.5)
    for (const rotation of Object.values(pose.joints)) {
      assert.ok(rotation.every(Number.isFinite))
    }
  }
})

test('reduced motion returns the representative pose', () => {
  assert.deepEqual(getStretchPose('chin-tuck', 0.1, true), getStretchPose('chin-tuck', 0.9, true))
})
```

**Step 2: Run tests and verify RED**

Run: `node --import tsx --test src/lib/stretchPoses.test.ts`

Expected: FAIL because pose functions do not exist.

**Step 3: Implement pose definitions**

Create typed neutral/start/end joint rotations for head, neck, shoulders, elbows, wrists, torso, hips and knees. Use a smooth sine cycle for continuous animation and a fixed midpoint for reduced motion.

**Step 4: Run tests and verify GREEN**

Run: `npm test`

Expected: all tests pass.

**Step 5: Install Three.js and implement the stage**

Run: `npm install three`

Build a local scene with rounded capsule/sphere body parts, a matte green outfit, warm skin material, orthographic camera, hemisphere/key/rim lights, transparent WebGL canvas and a soft contact-shadow plane. Expose:

```ts
export type StretchScene = {
  setExercise(id: StretchExerciseId): void
  setReducedMotion(value: boolean): void
  dispose(): void
}

export function createStretchScene(container: HTMLElement, initialId: StretchExerciseId): StretchScene
```

The animation loop calls `getStretchPose`, applies joint rotations, resizes to the container, and disposes all geometry, material, renderer, observers and animation frames on teardown.

**Step 6: Build and commit**

Run: `npm run build`

Expected: TypeScript and Vite build pass.

```bash
git add package.json package-lock.json src/lib/stretchPoses.ts src/lib/stretchPoses.test.ts src/lib/stretchScene.ts
git commit -m "feat: render procedural 3D stretch coach"
```

### Task 3: React long-break trainer

**Files:**
- Create: `src/components/StretchTrainer3D.tsx`
- Modify: `src/App.tsx`
- Modify: `src/styles.css`

**Step 1: Add a failing routine integration test**

Extend `src/lib/stretchRoutine.test.ts` to prove a paused countdown does not advance the automatic exercise, while a manual offset still changes it.

**Step 2: Run the focused test and verify RED**

Run: `node --import tsx --test src/lib/stretchRoutine.test.ts`

Expected: FAIL until the routine accepts an explicit elapsed value or equivalent stable countdown input.

**Step 3: Implement the React trainer**

`StretchTrainer3D` owns only the manual offset and scene lifecycle. It receives `remaining`, `duration`, and `running`, derives the active step, and renders:

```tsx
<section className="stretch-trainer" aria-label="大休息拉伸训练">
  <div ref={stageRef} className="stretch-stage" aria-hidden="true" />
  <div className="stretch-copy">
    <span>{step.exercise.focus}</span>
    <h2>{step.exercise.title}</h2>
    <p>{step.exercise.cue}</p>
    <p className="stretch-safety">{step.exercise.safety}</p>
  </div>
  <nav aria-label="切换拉伸动作">…</nav>
</section>
```

Render it only when `phase === 'long'`. Keep the existing illustration and guidance for short breaks. Pass `settings.longDuration * 60` as the routine duration and keep the existing main countdown, postpone, pause, skip, and strict-break controls intact.

**Step 4: Style responsive and reduced-motion states**

Add a two-column long-break layout on desktop, a compact stacked layout on small screens, action dots, current-action progress, 3D fallback artwork, dark-theme colors, and `prefers-reduced-motion` transition overrides.

**Step 5: Verify and commit**

Run: `npm test && npm run build`

Expected: all tests pass and the app builds.

```bash
git add src/components/StretchTrainer3D.tsx src/App.tsx src/styles.css src/lib/stretchRoutine.test.ts src/lib/stretchRoutine.ts
git commit -m "feat: guide long breaks with 3D stretches"
```

### Task 4: Tauri full-screen trainer and Vite multi-page output

**Files:**
- Create: `break.html`
- Create: `src/break.ts`
- Create: `src/break.css`
- Modify: `vite.config.ts`
- Delete: `public/break.html`
- Delete: `public/break.js`
- Delete: `public/break.css`

**Step 1: Add a failing build assertion**

Add a `scripts/verify-build.mjs` check that requires both `dist/index.html` and `dist/break.html`, and verifies that `dist/break.html` references a bundled module under `assets/`; then add `verify:build` to `package.json`.

**Step 2: Run the assertion and verify RED**

Run: `npm run build && npm run verify:build`

Expected: FAIL because the current copied break page references the unbundled classic `break.js` script instead of a generated module under `assets/`.

**Step 3: Convert the break page to a Vite entry**

Configure named `index.html` and `break.html` Rollup inputs. The new module imports the shared routine and scene, listens to `repose-break-status`, and updates the same title, timer, total progress, postpone controls and strict-break text as today.

For `phase === 'long'`, show the 3D trainer with step progress and manual previous/next controls. For a short break, hide the trainer and retain the eye-rest presentation. Derive the active exercise from `payload.duration` and `payload.remaining`, so every monitor stays synchronized.

**Step 4: Add full-screen responsive styles**

Use a centered two-column stage on wide displays and a stacked presentation on compact windows. Preserve current CSP and local-only assets.

**Step 5: Verify and commit**

Run: `npm test && npm run build && npm run verify:build`

Expected: all tests pass and both HTML entry points exist with hashed local assets.

```bash
git add break.html src/break.ts src/break.css vite.config.ts scripts/verify-build.mjs package.json public/break.html public/break.js public/break.css
git commit -m "feat: add 3D stretches to strict break screen"
```

### Task 5: Final verification and product documentation

**Files:**
- Modify: `README.md`
- Modify: `README-rust.md`

**Step 1: Document the user-visible behavior**

Explain that long breaks include eight offline 3D guided stretches, with shoulder/neck emphasis, automatic cycling, manual navigation, reduced-motion behavior and WebGL fallback.

**Step 2: Run complete verification**

Run: `npm test`

Expected: all tests pass.

Run: `npm run build`

Expected: TypeScript and both Vite pages build without warnings.

Run: `cargo test --manifest-path src-tauri/Cargo.toml`

Expected: Rust tests pass.

**Step 3: Inspect the browser UI**

Run the local preview, start a long break, and verify automatic cycling, manual controls, resizing, dark mode and reduced motion. Verify the built `break.html` renders the long-break trainer with a representative status event or Tauri development run.

**Step 4: Commit documentation**

```bash
git add README.md README-rust.md
git commit -m "docs: describe guided 3D long breaks"
```
