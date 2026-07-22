import { resolve } from 'node:path';
import { dts as rolldownDts } from 'rolldown-plugin-dts';
import simplei18n from '@simplei18n/core/vite';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

const dts = (...args: Parameters<typeof rolldownDts>) =>
  rolldownDts(...args).map(
    (plugin) => plugin.name.endsWith("fake-js") ? { ...plugin, enforce: "pre" } : plugin
  );

export default defineConfig(({ mode }) => ({
  build: {
    outDir: 'dist/app',
    ...(mode === 'lib' && {
      outDir: 'dist/lib',
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

  oxc: {
    exclude: [/\.js$/, /\.d\.[cm]?ts$/],
  },

  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },
  plugins: [
    ...(mode === 'lib' ? [
      dts({ generator: 'tsgo' })
    ] : []),
    react(),
    tailwindcss(),
    simplei18n(),
  ],
}));
