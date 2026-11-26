import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// The build goes into the Rust crate (src/app/ui/dist), which the binary
// embeds and serves as /, /app.js and /app.css. Fixed names, no inline
// scripts: the page's CSP only allows its own files.
//
// `npm run dev` serves the page with hot reload and forwards the API to a
// running `keymule ui` (see ui/README.md).
export default defineConfig({
  plugins: [react()],
  base: '/',
  build: {
    outDir: '../src/app/ui/dist',
    emptyOutDir: true,
    modulePreload: { polyfill: false },
    cssCodeSplit: false,
    sourcemap: false,
    rollupOptions: {
      output: {
        entryFileNames: 'app.js',
        chunkFileNames: 'app-[name].js',
        assetFileNames: 'app[extname]',
        inlineDynamicImports: true,
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: `http://127.0.0.1:${process.env.KEYMULE_UI_PORT || 7420}`, changeOrigin: true },
      '/login': { target: `http://127.0.0.1:${process.env.KEYMULE_UI_PORT || 7420}`, changeOrigin: true },
    },
  },
})
