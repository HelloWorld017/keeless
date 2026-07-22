import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 1420,
    strictPort: true,
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: process.env.TAURI_ENV_PLATFORM === 'windows' ? 'chrome113' : 'safari17',
    minify: process.env.TAURI_ENV_DEBUG ? false : 'oxc',
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
  },
});
