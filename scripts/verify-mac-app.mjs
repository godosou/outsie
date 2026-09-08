import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const app = path.resolve(root, process.argv[2] || 'src-tauri/target/release/bundle/macos/Repose.app')
const config = JSON.parse(await readFile(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
const plist = JSON.parse(execFileSync('/usr/bin/plutil', [
  '-convert', 'json', '-o', '-', path.join(app, 'Contents/Info.plist'),
], { encoding: 'utf8' }))

assert.ok(plist.CFBundleIconFile, 'Packaged App has no CFBundleIconFile: Dock cannot load the icon')
assert.equal(path.basename(plist.CFBundleIconFile), plist.CFBundleIconFile)
const iconName = plist.CFBundleIconFile.endsWith('.icns') ? plist.CFBundleIconFile : `${plist.CFBundleIconFile}.icns`
const bundled = await readFile(path.join(app, 'Contents/Resources', iconName))
assert.equal(bundled.subarray(0, 4).toString(), 'icns', 'Packaged Dock icon must be an ICNS file')
assert.deepEqual(bundled, await readFile(path.join(root, 'src-tauri/icons/icon.icns')), 'Packaged Dock icon is stale')
assert.equal(plist.CFBundleDisplayName, config.productName)
assert.equal(plist.CFBundleShortVersionString, config.version)
assert.equal(plist.CFBundleIdentifier, config.identifier)
console.log(`Verified ${config.productName} ${config.version}: bundle identity and embedded Dock icon match the source.`)
