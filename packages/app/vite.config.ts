import { readFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import simplei18n from '@simplei18n/core/vite';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { dts as rolldownDts } from 'rolldown-plugin-dts';
import { defineConfig, esmExternalRequirePlugin } from 'vite';
import type { Plugin } from 'vite';

const dts = (...args: Parameters<typeof rolldownDts>) =>
  rolldownDts(...args).map(plugin =>
    plugin.name.endsWith('fake-js') ? { ...plugin, enforce: 'pre' } : plugin,
  );

const asset = (): Plugin => {
  return {
    name: 'vite-plugin-emit-asset',
    enforce: 'pre',

    async load(id) {
      if (!id.endsWith('?asset')) {
        return null;
      }

      const file = id.replace(/\?.*$/, '');
      const referenceId = this.emitFile({
        type: 'asset',
        name: basename(file),
        source: await readFile(file),
      });

      return `
        export default import.meta.ROLLUP_FILE_URL_${referenceId};
      `;
    },
  };
};

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
    }),
  },

  oxc: {
    exclude: [/\.js$/, /\.d\.[cm]?ts$/],
  },

  define: {
    ...(mode !== 'lib' && {
      __KEELESS_BROWSER_HOST_DISABLED__: 'false',
    }),
  },

  resolve: {
    alias: {
      '@': resolve(__dirname, 'src'),
    },
  },

  plugins: [
    ...(mode === 'lib'
      ? [
          asset(),
          dts({ generator: 'tsgo' }),
          esmExternalRequirePlugin({
            external: [/^react(?:\/.*)?$/, /^react-dom(?:\/.*)?$/],
          }),
        ]
      : []),
    react(),
    tailwindcss(),
    simplei18n(),
  ],
}));
