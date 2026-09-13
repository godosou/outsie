import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync, existsSync } from 'node:fs'

const root = new URL('../../', import.meta.url)
const read = (path: string) => readFileSync(new URL(path, root), 'utf8')

const brand = JSON.parse(read('brand.json')) as {
  name: string
  wordmark: string
  tagline: string
  keep: Record<string, string>
}

/**
 * Every place the product's name is written down, and how to read it.
 *
 * This list is the point of the file. The last rename touched forty files by
 * hand and the only thing keeping it honest was a grep — and a grep finds what
 * you thought to look for. Renaming the product is now: edit brand.json, run
 * this test, and fix whatever it names. If a new declaration site appears and
 * is not added here, the next rename will miss it exactly the way the last one
 * nearly did.
 */
const declarationSites: Array<{ file: string; extract: (text: string) => string }> = [
  { file: 'package.json', extract: t => JSON.parse(t).productName },
  { file: 'src-tauri/tauri.conf.json', extract: t => JSON.parse(t).productName },
  // The executable's name, not just the bundle's. macOS attributes a TCC
  // prompt -- Bluetooth, camera, anything -- to the executable, so with the
  // Cargo default the Bluetooth dialog read 「"repose" would like to use
  // Bluetooth」: a name nobody installed, asking for a permission. Same class
  // as the authorization box that used to say "osascript".
  { file: 'src-tauri/tauri.conf.json', extract: t => JSON.parse(t).mainBinaryName },
  {
    file: 'src-tauri/src/unlock.rs',
    extract: t => t.match(/pub const BRAND: &str = "([^"]+)"/)?.[1] ?? '',
  },
  {
    file: 'tools/ble-spike/android/app/src/main/kotlin/ai/repose/blespike/Brand.kt',
    extract: t => t.match(/const val NAME = "([^"]+)"/)?.[1] ?? '',
  },
  {
    file: 'electron/main.cjs',
    extract: t => t.match(/app\.setName\('([^']+)'\)/)?.[1] ?? '',
  },
]

test('every declaration of the product name agrees with brand.json', () => {
  for (const { file, extract } of declarationSites) {
    assert.equal(
      extract(read(file)),
      brand.name,
      `${file} does not say ${brand.name} — brand.json is the source of truth`,
    )
  }
})

test('the wordmark is declared once per platform, not spelled out inline', () => {
  const kotlin = read('tools/ble-spike/android/app/src/main/kotlin/ai/repose/blespike/Brand.kt')
  assert.match(kotlin, new RegExp(`const val WORDMARK = "${brand.wordmark.replace('.', '\\.')}"`))
  // React derives it; break.html is a static shell with nothing to import, so
  // there the wordmark is literal. That is fine as long as it is literally right.
  assert.match(read('src/App.tsx'), /BRAND_WORDMARK/)
  assert.ok(
    read('break.html').includes(brand.wordmark.replace(/\.$/, '')),
    'break.html does not carry the wordmark',
  )
})

test('the window title is the name and the tagline', () => {
  const config = JSON.parse(read('src-tauri/tauri.conf.json'))
  assert.equal(config.app.windows[0].title, `${brand.name} · ${brand.tagline}`)
})

/**
 * The other half of a rename, and the half that breaks things silently.
 *
 * These strings are on disk, on the air, or inside an HMAC pre-image shared
 * with the phone. Sweeping them along with the product name would orphan an
 * installed authorization plugin (plugin.c's PERMIT_PATH is a compile-time
 * macro, so editing it changes the cdhash), or make every beacon fail to
 * verify — which on screen looks exactly like a phone that is out of range.
 *
 * brand.json records them so the next person can see what was left behind on
 * purpose; this asserts they are still there.
 */
test('identifiers, paths and protocol labels survive a rename untouched', () => {
  const mustContain: Array<[keyof typeof brand.keep, string]> = [
    ['bundleIdentifier', 'src-tauri/tauri.conf.json'],
    ['androidPackage', 'tools/ble-spike/android/app/build.gradle.kts'],
    ['keyDirectory', 'src-tauri/src/unlock.rs'],
    ['beaconLabel', 'tools/ble-spike/mac/presence-verify.swift'],
    ['electronUserDataDirectory', 'electron/main.cjs'],
  ]
  for (const [key, file] of mustContain) {
    assert.ok(
      read(file).includes(brand.keep[key]),
      `${file} no longer contains ${brand.keep[key]} — a rename swept an identifier`,
    )
  }

  // The beacon label must be byte-identical on both sides of the radio. One
  // end changing is the failure that looks like nothing at all.
  const phoneLabel = read(
    'tools/ble-spike/android/app/src/main/kotlin/ai/repose/blespike/SpikeContract.kt',
  ).match(/PRESENCE_BEACON_LABEL = "([^"]+)"/)?.[1]
  assert.equal(phoneLabel, brand.keep.beaconLabel, 'the phone and the Mac disagree on the beacon label')
})

test('the app embeds a Dock icon and shares the vector mark across the interface', () => {
  const config = JSON.parse(read('src-tauri/tauri.conf.json'))
  assert.ok(config.bundle.icon.includes('icons/icon.icns'))
  for (const path of config.bundle.icon) assert.ok(existsSync(new URL(`src-tauri/${path}`, root)))
  assert.match(read('src/App.tsx'), /<img src="\.\/favicon\.svg"/)
  assert.match(read('break.html'), /<img src="\/favicon\.svg"/)
})
