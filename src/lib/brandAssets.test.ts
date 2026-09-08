import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync, existsSync } from 'node:fs'

const root = new URL('../../', import.meta.url)
const read = (path: string) => readFileSync(new URL(path, root), 'utf8')

test('all current user-visible app and release names are Repose', () => {
  const config = JSON.parse(read('src-tauri/tauri.conf.json'))
  const metadata = JSON.parse(read('package.json'))
  assert.equal(config.productName, 'Repose')
  assert.equal(metadata.productName, 'Repose')
  for (const path of ['src-tauri/tauri.conf.json', 'scripts/package-tauri-mac.mjs', 'README.md', 'README-rust.md']) {
    assert.doesNotMatch(read(path), /Repose Lite|Repose-Lite/)
  }
})

test('the app embeds a Dock icon and shares the vector mark across the interface', () => {
  const config = JSON.parse(read('src-tauri/tauri.conf.json'))
  assert.ok(config.bundle.icon.includes('icons/icon.icns'))
  for (const path of config.bundle.icon) assert.ok(existsSync(new URL(`src-tauri/${path}`, root)))
  assert.match(read('src/App.tsx'), /<img src="\.\/favicon\.svg"/)
  assert.match(read('break.html'), /<img src="\/favicon\.svg"/)
})
