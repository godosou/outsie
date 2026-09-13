import { spawnSync } from 'node:child_process'
import { access, copyFile, mkdir, mkdtemp, readFile, rm, symlink, writeFile } from 'node:fs/promises'
import { createHash } from 'node:crypto'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { ensureIdentity } from './dev-signing-identity.mjs'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const metadata = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
const config = JSON.parse(await readFile(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
if (process.platform !== 'darwin') throw new Error('macOS is required to build this release.')
if (!/^\d+\.\d+\.\d+$/.test(metadata.version) || metadata.version !== config.version) throw new Error('Release versions must match.')
const name = `Outsie-${metadata.version}-mac-${process.arch}`
const release = path.join(root, 'release', name)
try {
  await access(release)
  throw new Error(`Release already exists; preserving it: ${release}`)
} catch (error) { if (error.code !== 'ENOENT') throw error }

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${command} exited with status ${result.status}`)
}

run('npm', ['run', 'package:mac'])
run('npm', ['run', 'verify:build'])
run('npm', ['run', 'verify:mac'])
const source = path.join(root, 'src-tauri/target/release/bundle/macos/Outsie.app')
const licenses = path.join(source, 'Contents/Resources/licenses')
await mkdir(licenses, { recursive: true })
await copyFile(path.join(root, 'src/assets/STRETCH-HUMAN-LICENSE.md'), path.join(licenses, 'Stretch-Human.md'))
await copyFile(path.join(root, 'node_modules/three/LICENSE'), path.join(licenses, 'Three-MIT.txt'))
// The same local identity the dev build uses. Ad-hoc ("-") would put the
// designated requirement back on a cdhash, and every release would cost the
// person a fresh Bluetooth grant.
const identity = ensureIdentity() || '-'
run('/usr/bin/codesign', ['--force', '--deep', '--sign', identity, source])
run('/usr/bin/codesign', ['--verify', '--deep', '--strict', source])
await mkdir(release, { recursive: true })
const app = path.join(release, 'Outsie.app')
run('/usr/bin/ditto', [source, app])
const stage = await mkdtemp(path.join(tmpdir(), 'repose-dmg-'))
const dmg = path.join(release, `${name}.dmg`)
try {
  run('/usr/bin/ditto', [app, path.join(stage, 'Outsie.app')])
  await symlink('/Applications', path.join(stage, 'Applications'))
  run('/usr/bin/hdiutil', ['create', '-volname', `Outsie ${metadata.version}`, '-srcfolder', stage, '-format', 'UDZO', '-fs', 'HFS+', dmg])
  run('/usr/bin/hdiutil', ['verify', dmg])
  const hash = createHash('sha256').update(await readFile(dmg)).digest('hex')
  await writeFile(path.join(release, 'SHA256SUMS.txt'), `${hash}  ${path.basename(dmg)}\n`)
  console.log(`App: ${app}\nDMG: ${dmg}\nSigned as ${identity === '-' ? 'ad hoc' : `"${identity}" (self-signed)`}; not Apple notarized.`)
} finally {
  // Only the unique staging directory created above; published releases stay intact.
  await rm(stage, { recursive: true, force: true })
}
