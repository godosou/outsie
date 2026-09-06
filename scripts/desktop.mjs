import { spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { setTimeout as delay } from 'node:timers/promises'

const root = fileURLToPath(new URL('../', import.meta.url))
const require = createRequire(import.meta.url)
const isDev = process.argv.includes('--dev')
const serverURL = 'http://127.0.0.1:47832'
let vite = null
let electronProcess = null
let stopping = false

function stop(exitCode = 0) {
  if (stopping) return
  stopping = true
  if (electronProcess && electronProcess.exitCode === null) electronProcess.kill('SIGTERM')
  if (vite && vite.exitCode === null) vite.kill('SIGTERM')
  process.exitCode = exitCode
}

process.on('SIGINT', () => stop(130))
process.on('SIGTERM', () => stop(143))

async function viteIsReady() {
  try {
    const response = await fetch(`${serverURL}/@vite/client`, { signal: AbortSignal.timeout(800) })
    if (!response.ok || !(response.headers.get('content-type') || '').includes('javascript')) return false
    const page = await fetch(serverURL, { signal: AbortSignal.timeout(800) })
    return page.ok && /<title>[^<]*Repose/i.test(await page.text())
  } catch {
    return false
  }
}

async function main() {
  let electronBinary
  try {
    electronBinary = require('electron')
    if (typeof electronBinary !== 'string' || !existsSync(electronBinary)) throw new Error('Electron binary missing')
  } catch {
    throw new Error('Electron 尚未安装完整。请先运行 npm install；如果运行文件下载未完成，请运行 npx install-electron --no 后重试。')
  }

  if (!isDev && !existsSync(path.join(root, 'dist', 'index.html'))) {
    throw new Error('尚未找到构建文件。请先运行 npm run build，再运行 npm run desktop。开发模式可运行 npm run desktop:dev。')
  }

  if (isDev && !await viteIsReady()) {
    const viteCLI = path.join(root, 'node_modules', 'vite', 'bin', 'vite.js')
    if (!existsSync(viteCLI)) throw new Error('未找到 Vite。请先运行 npm install。')
    vite = spawn(process.execPath, [viteCLI, '--host', '127.0.0.1', '--port', '47832', '--strictPort'], {
      cwd: root,
      stdio: 'inherit',
      env: process.env,
    })
    let viteError = null
    vite.on('error', error => { viteError = error })
    let ready = false
    for (let attempt = 0; attempt < 100 && !stopping; attempt += 1) {
      if (viteError) throw viteError
      if (vite.exitCode !== null) throw new Error('Vite 未能启动。请检查 47832 端口是否被其他程序占用。')
      if (await viteIsReady()) { ready = true; break }
      await delay(250)
    }
    if (stopping) return
    if (!ready) throw new Error('等待 Vite 启动超时。请检查上方输出后重试。')
  }

  if (stopping) return
  const electronEnv = { ...process.env }
  // A parent process may use Electron as Node; the desktop child must launch its GUI.
  delete electronEnv.ELECTRON_RUN_AS_NODE
  electronProcess = spawn(electronBinary, [path.join(root, 'electron', 'main.cjs'), ...(isDev ? ['--repose-dev'] : [])], {
    cwd: root,
    stdio: 'inherit',
    env: electronEnv,
  })
  electronProcess.on('error', error => {
    console.error(`无法启动 Repose：${error.message}`)
    stop(1)
  })
  electronProcess.on('exit', (code, signal) => stop(code ?? (signal ? 1 : 0)))
  if (vite) vite.on('exit', () => { if (!stopping) stop(1) })
}

main().catch(error => {
  console.error(`\n${error.message}\n`)
  stop(1)
})
