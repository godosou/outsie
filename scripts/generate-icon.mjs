import { copyFile } from 'node:fs/promises'

// The desktop packagers share the 1024 px raster generated from public/favicon.svg.
const output = process.argv[2]
if (!output) throw new Error('Usage: node scripts/generate-icon.mjs /path/to/icon.png')
await copyFile(new URL('../src-tauri/icons/ios/AppIcon-512@2x.png', import.meta.url), output)
