// `npm run package:mac`: build the app, then sign it with the local dev identity.
//
// Signing happens AFTER `tauri build`, on the finished bundle, the same way the
// release script does — not through Tauri's APPLE_SIGNING_IDENTITY path, which
// also switches on the hardened runtime and a notarization branch this app has
// never run under. The helper binaries under Resources/scripts are sealed as
// resources by the bundle signature (--deep only descends into nested code
// locations), which is all --verify --deep --strict needs; TCC attributes
// their Bluetooth use to the parent app, so their own signature is moot.
//
// With no identity available the build is left ad-hoc, and says so.
import { spawnSync } from 'node:child_process'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { ensureIdentity } from './dev-signing-identity.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
function run(cmd, args) {
  const r = spawnSync(cmd, args, { cwd: root, stdio: 'inherit' })
  if (r.error) throw r.error
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(' ')} exited ${r.status}`)
}

run('npx', ['tauri', 'build', '--bundles', 'app'])

const app = path.join(root, 'src-tauri/target/release/bundle/macos/Outsie.app')
const identity = ensureIdentity()
if (!identity) {
  console.warn('package:mac: no signing identity — bundle left ad-hoc; macOS will ask for Bluetooth again after every rebuild')
} else {
  // The command line beside the app: built here, carried inside the bundle,
  // signed with it (design doc §07). `outsie shortcuts` reads and changes the
  // same file the panel edits.
  run('cargo', ['build', '--release', '--bin', 'outsie', '--manifest-path', 'src-tauri/Cargo.toml'])
  const { copyFileSync } = await import('node:fs')
  // NOT `Contents/MacOS/outsie`: the app binary is `Outsie`, and the default
  // APFS volume is case-insensitive, so that path is the same file. The first
  // build that tried it shipped a bundle whose main executable was the CLI.
  copyFileSync(path.join(root, 'src-tauri/target/release/outsie'), path.join(app, 'Contents/MacOS/outsie-cli'))
  run('/usr/bin/codesign', ['--force', '--deep', '--sign', identity, app])
  run('/usr/bin/codesign', ['--verify', '--deep', '--strict', app])
  const r = spawnSync('/usr/bin/codesign', ['-d', '-r-', app], { encoding: 'utf8' })
  console.log(`package:mac: signed as "${identity}"\n${(r.stderr || r.stdout || '').trim()}`)
}
