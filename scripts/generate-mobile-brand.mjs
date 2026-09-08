import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { deflateSync } from 'node:zlib'

// The same four-petal geometry as public/favicon.svg. Generate platform assets
// at their actual pixel sizes; iOS artwork is opaque and lets the OS mask it.
const root = resolve(import.meta.dirname, '..')
const rotations = [-40, 40, 130, 220].map(angle => [Math.cos(angle * Math.PI / 180), Math.sin(angle * Math.PI / 180)])
const crcTable = Array.from({ length: 256 }, (_, index) => {
  let value = index
  for (let bit = 0; bit < 8; bit++) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1
  return value >>> 0
})
function chunk(type, data) {
  const label = Buffer.from(type)
  let crc = 0xffffffff
  for (const byte of Buffer.concat([label, data])) crc = crcTable[(crc ^ byte) & 255] ^ (crc >>> 8)
  const header = Buffer.alloc(4)
  header.writeUInt32BE(data.length)
  const checksum = Buffer.alloc(4)
  checksum.writeUInt32BE((crc ^ 0xffffffff) >>> 0)
  return Buffer.concat([header, label, data, checksum])
}
function png(size) {
  const pixels = Buffer.alloc((size * 3 + 1) * size)
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    const rgb = [0, 0, 0]
    for (let sy = 0; sy < 4; sy++) for (let sx = 0; sx < 4; sx++) {
      const dx = ((x + (sx + .5) / 4) / size * 64 - 32) / .78
      const dy = ((y + (sy + .5) / 4) / size * 64 - 32) / .78
      const petal = dx * dx + dy * dy > 25 && rotations.some(([c, s]) => {
        const lx = dx * c + dy * s
        const ly = -dx * s + dy * c
        return (lx / 7) ** 2 + ((ly + 10) / 15) ** 2 <= 1
      })
      const color = petal ? [226, 236, 207] : [47, 81, 61]
      color.forEach((value, i) => { rgb[i] += value / 16 })
    }
    rgb.forEach((v, i) => { pixels[y * (size * 3 + 1) + 1 + x * 3 + i] = Math.round(v) })
  }
  const header = Buffer.alloc(13)
  header.writeUInt32BE(size); header.writeUInt32BE(size, 4)
  header[8] = 8; header[9] = 2
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', header), chunk('IDAT', deflateSync(pixels)), chunk('IEND', Buffer.alloc(0))])
}
const ios = 'mobile/ios/Runner/Assets.xcassets/AppIcon.appiconset'
const catalog = JSON.parse(await readFile(resolve(root, ios, 'Contents.json'), 'utf8'))
const assets = new Map(catalog.images.map(image => [resolve(root, ios, image.filename), parseFloat(image.size) * parseFloat(image.scale)]))
for (const scale of [1, 2, 3]) {
  assets.set(resolve(root, `mobile/ios/Runner/Assets.xcassets/LaunchImage.imageset/LaunchImage${scale === 1 ? '' : `@${scale}x`}.png`), 80 * scale)
}
for (const [density, size] of Object.entries({ mdpi: 48, hdpi: 72, xhdpi: 96, xxhdpi: 144, xxxhdpi: 192 })) {
  assets.set(resolve(root, `mobile/android/app/src/main/res/mipmap-${density}/ic_launcher.png`), size)
}
for (const [path, size] of assets) {
  await mkdir(dirname(path), { recursive: true })
  await writeFile(path, png(size))
}
console.log(`Generated ${assets.size} Repose launcher assets from the shared brand geometry.`)
