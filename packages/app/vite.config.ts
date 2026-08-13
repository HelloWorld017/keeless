import { resolve } from 'node:path';
import simplei18n from '@simplei18n/core/vite';
import tailwindcss from '@tailwindcss/vite';
import react from '@vitejs/plugin-react';
import { dts as rolldownDts } from 'rolldown-plugin-dts';
import { defineConfig, esmExternalRequirePlugin } from 'vite';

const dts = (...args: Parameters<typeof rolldownDts>) =>
  rolldownDts(...args).map(plugin =>
    plugin.name.endsWith('fake-js') ? { ...plugin, enforce: 'pre' } : plugin,
  );

export default defineConfig(({ mode, command }) => ({
  build: {
    outDir: 'dist/app',
    ...(mode === 'lib' && {
      outDir: 'dist/lib',
      lib: {
        entry: resolve(__dirname, 'src/index.ts'),
        fileName: 'index',
        cssFileName: 'styles',
        emitAssets: true,
        formats: ['es'],
      },
      rolldownOptions: {
        output: {
          assetFileNames: asset =>
            asset.names.find(name => name.endsWith('.css'))
              ? '[name].[ext]'
              : 'assets/[name].[ext]',
          chunkFileNames: 'assets/[name].js',
        },
      },
    }),
  },

  oxc: {
    exclude: [/\.js$/, /\.d\.[cm]?ts$/],
  },

  define: {
    ...(command === 'serve' && {
      __DEV__: 'true',
    }),
    ...(mode !== 'lib' && {
      __DEV__: 'false',
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
