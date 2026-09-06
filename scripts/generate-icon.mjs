import { writeFile } from 'node:fs/promises'
import { deflateSync } from 'node:zlib'

// Rasterize the favicon's four-petal motif at 1024 px with transparent corners.
// Analytic shape edges keep this reproducible without graphics dependencies.
const output = process.argv[2]
if (!output) throw new Error('Usage: node scripts/generate-icon.mjs /path/to/icon.png')
const size = 1024
const pixels = Buffer.alloc((size * 4 + 1) * size)
const mint = [226, 236, 207]
const center = [47, 81, 61]
const rotations = [-40, 40, 130, 220].map((angle) => {
  const radians = angle * Math.PI / 180
  return [Math.cos(radians), Math.sin(radians)]
})
const clamp = (value) => Math.min(1, Math.max(0, value))

function roundedSquareDistance(x, y) {
  const dx = Math.abs(x - 512) - 238
  const dy = Math.abs(y - 512) - 238
  return Math.hypot(Math.max(dx, 0), Math.max(dy, 0)) + Math.min(Math.max(dx, dy), 0) - 192
}

function over(pixel, rgb, alpha) {
  const opacity = alpha + pixel[3] * (1 - alpha)
  if (!opacity) return
  for (let channel = 0; channel < 3; channel += 1) {
    pixel[channel] = (rgb[channel] * alpha + pixel[channel] * pixel[3] * (1 - alpha)) / opacity
  }
  pixel[3] = opacity
}

for (let y = 0; y < size; y += 1) {
  for (let x = 0; x < size; x += 1) {
    const px = x + 0.5
    const py = y + 0.5
    const shadowDistance = Math.max(0, roundedSquareDistance(px, py - 12))
    const pixel = [15, 28, 21, 0.2 * Math.exp(-(shadowDistance ** 2) / (2 * 17 ** 2))]
    const backgroundAlpha = clamp(0.5 - roundedSquareDistance(px, py))
    const gradient = clamp((py - 82) / 860)
    over(pixel, [70 - 29 * gradient, 99 - 30 * gradient, 73 - 21 * gradient], backgroundAlpha)
    for (const [cosine, sine] of rotations) {
      const dx = px - 512
      const dy = py - 512
      const localX = dx * cosine + dy * sine
      const localY = -dx * sine + dy * cosine
      const ellipseDistance = (Math.hypot(localX / 97, (localY + 152) / 205) - 1) * 97
      over(pixel, mint, clamp(0.5 - ellipseDistance))
    }
    over(pixel, center, clamp(64.5 - Math.hypot(px - 512, py - 512)))
    const offset = y * (size * 4 + 1) + 1 + x * 4
    for (let channel = 0; channel < 3; channel += 1) pixels[offset + channel] = Math.round(pixel[channel])
    pixels[offset + 3] = Math.round(pixel[3] * 255)
  }
}

const crcTable = Array.from({ length: 256 }, (_, index) => {
  let value = index
  for (let bit = 0; bit < 8; bit += 1) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1
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

const header = Buffer.alloc(13)
header.writeUInt32BE(size, 0)
header.writeUInt32BE(size, 4)
header[8] = 8 // 8-bit channels.
header[9] = 6 // RGBA.
await writeFile(output, Buffer.concat([
  Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk('IHDR', header),
  chunk('IDAT', deflateSync(pixels)),
  chunk('IEND', Buffer.alloc(0)),
]))
