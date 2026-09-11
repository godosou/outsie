import { spawnSync } from 'node:child_process'
import { access, copyFile, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

// Local macOS packaging using Electron's documented prebuilt-binary layout.
// No Apple signing identity, installation, or permissions changes are made.
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const source = path.join(root, 'node_modules/electron/dist/Electron.app')
const release = path.join(root, 'release')
// A versioned filename lets an update coexist with a currently running bundle.
const outputNameIndex = process.argv.indexOf('--output-name')
const outputName = outputNameIndex === -1 ? 'Outsie.app' : process.argv[outputNameIndex + 1]
if (!/^Outsie(?:-\d+\.\d+\.\d+)?\.app$/.test(outputName || '')) {
  throw new Error('--output-name must be Outsie.app or Outsie-<version>.app')
}
const output = path.join(release, outputName)
const productName = 'Outsie'
const bundleId = 'ai.repose.desktop'

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit', ...options })
  if (result.error) throw result.error
  if (result.status !== 0) throw new Error(`${path.basename(command)} exited with status ${result.status}`)
  return result
}

async function requireFile(file, explanation) {
  try { await access(file) } catch { throw new Error(explanation) }
}

function setPlist(plist, key, value, type = '-string') {
  run('/usr/bin/plutil', ['-replace', key, type, String(value), plist])
}

async function writeIconContainer(iconset, destination) {
  // Modern macOS accepts PNG payloads in ICNS. Writing the container directly
  // avoids iconutil's dependency on system image services outside the sandbox.
  const entries = [
    ['icp4', 'icon_16x16.png'],
    ['icp5', 'icon_32x32.png'],
    ['icp6', 'icon_32x32@2x.png'],
    ['ic07', 'icon_128x128.png'],
    ['ic08', 'icon_256x256.png'],
    ['ic09', 'icon_512x512.png'],
    ['ic10', 'icon_512x512@2x.png'],
    ['ic11', 'icon_16x16@2x.png'],
    ['ic12', 'icon_32x32@2x.png'],
    ['ic13', 'icon_128x128@2x.png'],
    ['ic14', 'icon_256x256@2x.png'],
  ]
  const chunks = []
  for (const [type, filename] of entries) {
    const data = await readFile(path.join(iconset, filename))
    const header = Buffer.alloc(8)
    header.write(type, 0, 4, 'ascii')
    header.writeUInt32BE(data.length + 8, 4)
    chunks.push(header, data)
  }
  const header = Buffer.alloc(8)
  header.write('icns', 0, 4, 'ascii')
  header.writeUInt32BE(8 + chunks.reduce((total, item) => total + item.length, 0), 4)
  await writeFile(destination, Buffer.concat([header, ...chunks]))
}

async function main() {
  if (process.platform !== 'darwin') throw new Error('Mac packaging must run on macOS.')
  await requireFile(source, 'Electron is not installed. Run npm install, then npm run package:mac.')
  await requireFile(path.join(root, 'dist/index.html'), 'The web app has not been built. Run npm run build first.')
  await requireFile(path.join(root, 'electron/main.cjs'), 'The Electron entry point electron/main.cjs is missing.')
  const bundledLicenses = [
    ['@fontsource-variable/manrope', 'Manrope-OFL.txt'],
    ['react', 'React-MIT.txt'],
    ['react-dom', 'ReactDOM-MIT.txt'],
    ['scheduler', 'ReactScheduler-MIT.txt'],
    ['lucide-react', 'Lucide-ISC.txt'],
  ]
  for (const [packageName] of bundledLicenses) {
    await requireFile(path.join(root, 'node_modules', packageName, 'LICENSE'),
      `The bundled dependency ${packageName} is missing its LICENSE file. Run npm install before packaging.`)
  }
  const metadata = JSON.parse(await readFile(path.join(root, 'package.json'), 'utf8'))
  const zipPath = path.join(release, `Outsie-${metadata.version}-mac-${process.arch}.zip`)
  const scratch = await mkdtemp(path.join(tmpdir(), 'repose-package-'))
  const staged = path.join(scratch, 'Outsie.app')

  try {
    console.log('Preparing Outsie.app…')
    run('/usr/bin/ditto', [source, staged])
    const contents = path.join(staged, 'Contents')
    const resources = path.join(contents, 'Resources')
    const appDirectory = path.join(resources, 'app')
    await rm(path.join(resources, 'default_app.asar'), { force: true })
    await rm(path.join(resources, 'electron.icns'), { force: true })
    await mkdir(appDirectory, { recursive: true })
    for (const license of ['LICENSE', 'LICENSES.chromium.html']) {
      await copyFile(path.join(root, 'node_modules/electron/dist', license), path.join(resources, license))
    }
    const licensesDirectory = path.join(resources, 'licenses')
    await mkdir(licensesDirectory, { recursive: true })
    for (const [packageName, filename] of bundledLicenses) {
      await copyFile(path.join(root, 'node_modules', packageName, 'LICENSE'), path.join(licensesDirectory, filename))
    }
    for (const folder of ['dist', 'electron']) {
      run('/usr/bin/ditto', [path.join(root, folder), path.join(appDirectory, folder)])
    }
    await writeFile(path.join(appDirectory, 'package.json'), `${JSON.stringify({
      name: 'repose', productName, version: metadata.version,
      description: metadata.description, main: 'electron/main.cjs',
      private: true,
    }, null, 2)}\n`)

    // Electron is bundled, so launching this app never needs Node.js or npm.
    const plist = path.join(contents, 'Info.plist')
    await rename(path.join(contents, 'MacOS/Electron'), path.join(contents, 'MacOS/Outsie'))
    for (const [key, value] of Object.entries({
      CFBundleDisplayName: productName,
      CFBundleName: productName,
      CFBundleExecutable: productName,
      CFBundleIdentifier: bundleId,
      CFBundleShortVersionString: metadata.version,
      CFBundleVersion: metadata.version,
      CFBundleIconFile: 'Outsie.icns',
      LSApplicationCategoryType: 'public.app-category.healthcare-fitness',
      NSAppleEventsUsageDescription: 'Outsie 会在电脑闲置时请求锁定 macOS 屏幕，以保护你的工作内容。',
    })) setPlist(plist, key, value)
    run('/usr/bin/plutil', ['-remove', 'ElectronAsarIntegrity', plist])

    // Rebrand helper metadata; Electron locates the original helper bundle names.
    const frameworks = path.join(contents, 'Frameworks')
    for (const entry of await readdir(frameworks)) {
      if (!entry.startsWith('Electron Helper') || !entry.endsWith('.app')) continue
      const helperPlist = path.join(frameworks, entry, 'Contents/Info.plist')
      const suffix = entry.match(/\(([^)]+)\)/)?.[1]?.toLowerCase()
      setPlist(helperPlist, 'CFBundleIdentifier', `${bundleId}.helper${suffix ? `.${suffix}` : ''}`)
      setPlist(helperPlist, 'CFBundleDisplayName', entry.replace('Electron', productName).replace(/\.app$/, ''))
      setPlist(helperPlist, 'CFBundleName', entry.replace('Electron', productName).replace(/\.app$/, ''))
      setPlist(helperPlist, 'CFBundleShortVersionString', metadata.version)
      setPlist(helperPlist, 'CFBundleVersion', metadata.version)
    }

    console.log('Drawing the Outsie Dock icon…')
    const iconset = path.join(scratch, 'Outsie.iconset')
    const icon = path.join(scratch, 'icon-1024.png')
    await mkdir(iconset)
    run(process.execPath, ['scripts/generate-icon.mjs', icon])
    for (const size of [16, 32, 128, 256, 512]) {
      for (const scale of [1, 2]) {
        const filename = path.join(iconset, `icon_${size}x${size}${scale === 2 ? '@2x' : ''}.png`)
        if (size * scale === 1024) await copyFile(icon, filename)
        else run('/usr/bin/sips', ['-z', String(size * scale), String(size * scale), icon, '--out', filename], { stdio: 'ignore' })
      }
    }
    await writeIconContainer(iconset, path.join(resources, 'Outsie.icns'))

    console.log('Applying a local ad hoc signature…')
    run('/usr/bin/codesign', ['--force', '--deep', '--sign', '-', staged])
    run('/usr/bin/codesign', ['--verify', '--deep', '--strict', staged])
    await mkdir(release, { recursive: true })
    // Build in scratch first, preserving the previous artifact if staging fails.
    await rm(output, { recursive: true, force: true })
    run('/usr/bin/ditto', [staged, output])
    await rm(zipPath, { force: true })
    run('/usr/bin/ditto', ['-c', '-k', '--sequesterRsrc', '--keepParent', output, zipPath])
    await copyFile(icon, path.join(release, 'Outsie-icon.png'))
    console.log(`\nApp: ${output}\nZIP: ${zipPath}\nLocally signed only; no Apple Developer signing or notarization.`)
  } finally {
    await rm(scratch, { recursive: true, force: true })
  }
}

main().catch((error) => {
  console.error(`Packaging failed: ${error.message}`)
  process.exitCode = 1
})
