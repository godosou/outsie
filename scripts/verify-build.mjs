import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'

const indexHtml = await readFile(new URL('../dist/index.html', import.meta.url), 'utf8')
const breakHtml = await readFile(new URL('../dist/break.html', import.meta.url), 'utf8')

assert.match(indexHtml, /assets\/[^"']+\.js/, 'main page should reference a bundled JavaScript asset')
assert.match(breakHtml, /type="module"/, 'break page should use the Vite module entry')
assert.match(breakHtml, /assets\/[^"']+\.js/, 'break page should reference a bundled JavaScript asset')
assert.doesNotMatch(breakHtml, /src="break\.js"/, 'break page should not use the legacy standalone script')

console.log('Verified main and break-page production entries.')
