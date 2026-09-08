import assert from 'node:assert/strict'
import test from 'node:test'
import { readFileSync } from 'node:fs'

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

test('one recognizable half-lidded mascot source is used across the interface', () => {
  const icon = read('public/favicon.svg')
  assert.match(icon, /data-repose-icon="smirk-flower-v2"/)
  assert.match(icon, /id="half-lidded-eyes"/)
  assert.match(icon, /id="knowing-smile"/)
  assert.match(read('src/App.tsx'), /<img src="\.\/favicon\.svg"/)
  assert.match(read('break.html'), /<img src="\/favicon\.svg"/)
})
