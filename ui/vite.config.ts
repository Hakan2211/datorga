import { defineConfig } from 'vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// Desktop app: Start runs in SPA mode. The prerendered shell becomes index.html for Tauri;
// there is no server at runtime, so no server functions. All data goes through Tauri commands.
export default defineConfig({
  clearScreen: false,
  server: { port: 3000, strictPort: true },
  resolve: { tsconfigPaths: true },
  plugins: [
    tailwindcss(),
    tanstackStart({ spa: { enabled: true, prerender: { outputPath: '/index.html' } } }),
    viteReact(),
  ],
})
