# Anatomical Stretch Guide Implementation Plan

**Goal:** Replace the rejected capsule character with a continuous, proportionate human and restrained anatomical teaching presentation inspired by Muscle & Motion.

**Architecture:** Convert MakeHuman CC0 core mesh and skin weights into a local, compact rigged asset. Share one Three.js renderer between the React break dialog and native full-screen break window. Highlight approximate surface regions, explicitly not individual anatomical muscles or a medical simulation.

**Tech Stack:** TypeScript, Three.js SkinnedMesh, React, Tauri, Node asset conversion and tests.

## Approved design

Keep the eight-exercise carousel, shoulder/neck/upper-back emphasis, pause and reduced-motion behavior. Use an ivory human, charcoal shorts, red target regions, neutral studio background and stable instructional camera angles. No Muscle & Motion media or branding is copied. Source assets are CC0; provenance is included.

## Implementation

1. Add an asset integrity test; run it before creating the asset. Convert only the body surface, exclude helper geometry, normalize four bone weights and validate joint hierarchy.
2. Replace segmented primitives with the local skinned body. Calibrate neutral arm positions, reuse semantic exercise joints, update highlights on each exercise and preserve renderer disposal, pause and reduced motion. Handle asset loading failure in both entry points.
3. Replace decorative green stage styling and toy fallback with a neutral anatomical-guide presentation and explicit region legend.
4. Run `npm test`, `npm run build`, `npm run verify:build`; inspect live previews for all eight poses, switching and small layout. Package macOS app, verify signing and native rendering. Record actual validation and limits.
