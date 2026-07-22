import { resolve } from 'node:path';
import simplei18n from '@simplei18n/core/vite';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

export default defineConfig(({ mode }) => ({
  build: {
    ...(mode === 'lib' && {
      lib: {
        entry: resolve(__dirname, 'src/index.ts'),
        fileName: 'index',
        cssFileName: 'styles',
        formats: ['es'],
      },

      rolldownOptions: {
        external: ['react', 'react-dom'],
      },
    }),
  },
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  plugins: [react(), tailwindcss(), simplei18n()],
}));
