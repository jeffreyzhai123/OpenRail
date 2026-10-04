import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

/** Where `cargo run -p sim-api` listens by default (its DEFAULT_PORT). */
const SIM_API = 'http://localhost:3000'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      // The app calls /api/...; sim-api's routes live at the root.
      '/api': {
        target: SIM_API,
        rewrite: (path) => path.replace(/^\/api/, ''),
      },
    },
  },
  test: {
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
  },
})
