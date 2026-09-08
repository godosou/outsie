import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

export default defineConfig({
  plugins: [react()],
  base: './',
  server: { host: '127.0.0.1', port: 47832, strictPort: true },
  build: {
    chunkSizeWarningLimit: 600,
    rollupOptions: {
      input: {
        main: resolve(process.cwd(), 'index.html'),
        break: resolve(process.cwd(), 'break.html'),
      },
    },
  },
})
