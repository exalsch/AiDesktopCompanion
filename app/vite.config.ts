import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// https://vite.dev/config/
export default defineConfig({
  // Important for Tauri builds so asset URLs (incl. WASM) resolve correctly
  base: './',
  plugins: [vue()],
  // Avoid listing JSON assets in optimizeDeps.include; Vite cannot prebundle JSON this way
  // Keep this empty unless a specific ESM dependency needs pre-bundling
  optimizeDeps: {
    include: [],
  },
  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes('node_modules')) return
          if (id.includes('@dqbd/tiktoken')) return 'vendor-tiktoken'
          if (id.includes('markdown-it')) return 'vendor-markdown'
          if (id.includes('highlight.js')) return 'vendor-highlight'
          if (id.includes('dompurify')) return 'vendor-dompurify'
          if (id.includes('@tauri-apps')) return 'vendor-tauri'
        },
      },
    },
  },
})
