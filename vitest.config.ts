import path from 'node:path'
import { defineConfig } from 'vitest/config'
import vue from '@vitejs/plugin-vue'

export default defineConfig({
  plugins: [vue()],
  resolve: {
    alias: [{
      find: /^klinecharts$/,
      replacement: path.resolve(__dirname, './node_modules/klinecharts/dist/index.esm.js'),
    }],
    dedupe: ['klinecharts'],
  },
  test: {
    environment: 'jsdom',
    include: ['tests/frontend/**/*.test.ts'],
  },
})
