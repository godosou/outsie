# Repose Brand Voice Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Ship a consistently named Repose app with one recognizable mascot icon and a rotating library of playful short-break prompts.

**Architecture:** Add a pure deterministic voice selector shared by React and the native break-page entry. Use one vector mascot source for the browser and application UI, then regenerate all Tauri platform icons from it. Keep the existing bundle identifier to preserve settings while changing all user-visible names.

**Tech Stack:** TypeScript, React, Tauri 2, SVG, Node tests and release scripts.

---

### Task 1: Add deterministic voice library

**Files:** create `src/lib/reposeVoice.ts`, `src/lib/reposeVoice.test.ts`; modify `src/App.tsx`, `src/break.ts`, `break.html`.

1. Write tests requiring at least six safe lines for each short-break context, deterministic selection by break ID and multiple selected variants across IDs.
2. Run the test and confirm it fails because the module is absent.
3. Implement typed copy pools and a stable string hash selector.
4. Use the selected line in the React short-break modal, native overlay, notification, postpone and completion feedback.
5. Run unit tests and commit.

### Task 2: Unify mascot and visible product name

**Files:** modify `public/favicon.svg`, `src/App.tsx`, `src/styles.css`, `break.html`, `src/break.css`, `scripts/generate-icon.mjs`, `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, package metadata and documentation.

1. Add an asset/name test that rejects user-visible `Repose Lite` strings and verifies the mascot files share the expected geometry markers.
2. Run it red, then change productName to Repose while preserving `ai.repose.lite` as the data identity.
3. Replace the old crossing petals with the half-lidded flower mark in the UI, favicon, Dock and menu bar.
4. Generate the Tauri icon set from the single SVG source and visually inspect 16px and 512px variants.
5. Run tests and commit.

### Task 3: Package and validate 0.6.0

**Files:** modify version metadata, `scripts/package-tauri-mac.mjs`, README and validation notes.

1. Update release packaging paths to `Repose.app` and `Repose-0.6.0-mac-arm64.dmg` without overwriting 0.5.0.
2. Run frontend and Rust tests, build and packaged native preview.
3. Verify App signature, bundle display name/version, DMG checksum, Applications link, icon and short-break copy rotation.
4. Merge the verified branch into local `main` and record outputs.
