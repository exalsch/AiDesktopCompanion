import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// https://vite.dev/config/
export default defineConfig({
  // Important for Tauri builds so asset URLs (incl. WASM) resolve correctly
  base: './',
  plugins: [vue()],
  optimizeDeps: {
    include: [
      '@dqbd/tiktoken/lite/init',
      '@dqbd/tiktoken/encoders/cl100k_base.json',
    ],
  },
})
