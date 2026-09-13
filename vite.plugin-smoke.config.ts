import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import tailwindcss from '@tailwindcss/vite'

const root = fileURLToPath(new URL('.', import.meta.url))

export default defineConfig({
  root,
  plugins: [vue(), tailwindcss()],
  cacheDir: 'target/plugin-smoke-vite-cache',
  server: {
    host: '127.0.0.1',
    port: 1430,
    strictPort: true,
    headers: {
      // Dev documents come from Vite, so enforce the same policy on their HTTP response.
      'Content-Security-Policy': "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost ws://127.0.0.1:1430; object-src 'none'; frame-src 'none'; base-uri 'none'; form-action 'none'",
    },
    watch: {
      // Anchor at this checkout: it may itself live inside another checkout's target/.
      ignored: ['src-tauri', 'target'].map(directory => `${root.replace(/\\/g, '/')}${directory}/**`),
    },
  },
  build: {
    outDir: 'target/plugin-smoke-dist',
    emptyOutDir: false,
    rollupOptions: { input: fileURLToPath(new URL('plugin-smoke.html', import.meta.url)) },
  },
})
