import { readFile } from 'node:fs/promises';
import { basename, resolve } from 'node:path';
import process from 'node:process';
import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';
import type { Plugin } from 'vite';
import {fileURLToPath} from 'node:url';

const dirname = fileURLToPath(new URL('.', import.meta.url));
const platform = process.env.PLATFORM ?? process.platform;
const env = process.env.NODE_ENV ?? 'development';

const ts = String.raw;

const napi = (): Plugin => ({
  name: 'vite-plugin-napi',
  async load(id) {
    if (id.endsWith('.node')) {
      const referenceId = this.emitFile({
        type: 'asset',
        name: basename(id),
        source: await readFile(id),
      });

      return {
        code: ts`
          const { fileURLToPath } = require('node:url');
          const assetPath = fileURLToPath(import.meta.ROLLUP_FILE_URL_${referenceId});
          module.exports = require(assetPath);
        `,
      };
    }
    return null;
  }
});

const binaryPrefix = 'binary:';
const binary = (): Plugin => ({
  name: 'vite-plugin-binary',
  async resolveId(source) {
    if (source.startsWith(binaryPrefix)) {
      return `\x00${source}`;
    }
  },
  async load(id) {
    if (!id.startsWith(`\x00${binaryPrefix}`)) {
      return;
    }

    const name = id.slice(binaryPrefix.length + 1);
    const profile = env === 'production' ? 'release' : 'debug';
    const executableSuffix = platform === 'win32' ? '.exe' : '';
    const assetPath = resolve(dirname, `../../target/${profile}/${name}${executableSuffix}`);

    const referenceId = this.emitFile({
      type: 'asset',
      name: `${name}${executableSuffix}`,
      source: await readFile(assetPath),
    });

    return ts`
      import { fileURLToPath } from 'node:url';
      export default fileURLToPath(import.meta.ROLLUP_FILE_URL_${referenceId});
    `;
  }
});

const assetPrefix = 'asset:';
const asset = (): Plugin => ({
  enforce: 'pre',
  name: 'vite-plugin-asset',
  async resolveId(source) {
    if (source.endsWith('?asset')) {
      return `\x00${assetPrefix}${source.slice(0, -6)}`;
    }
  },
  async load(id) {
    if (!id.startsWith(`\x00${assetPrefix}`)) {
      return;
    }

    const path = id.slice(assetPrefix.length + 1);
    const referenceId = this.emitFile({
      type: 'asset',
      name: basename(path),
      source: await readFile(path),
    });

    return ts`
      import { fileURLToPath } from 'node:url';
      export default fileURLToPath(import.meta.ROLLUP_FILE_URL_${referenceId});
    `;
  }
});

export default defineConfig(({ mode, isSsrBuild }) => {
  const isMain = !!isSsrBuild;
  const isPreload = !isMain && mode === 'preload';

  return {
    build: {
      lib: isPreload && {
        entry: './src/renderer/preload.ts',
        fileName: 'index',
        formats: ['cjs'],
      },
      rolldownOptions: {
        ...(isMain && { input: './src/index.ts' }),
        output: {
          ...(isMain && { assetFileNames: 'assets/[name][extname]' })
        },
        external: ['electron'],
      },
      ssrEmitAssets: true,
      outDir: isMain ? 'dist/main' : isPreload ? 'dist/preload' : 'dist/renderer',
    },

    define: {
      __ENV__: JSON.stringify(env),
      __PLATFORM__: JSON.stringify(platform),
      __KEELESS_BROWSER_HOST_DISABLED__: 'true',
      'process.env.NODE_ENV': JSON.stringify(env)
    },

    resolve: {
      alias: {
        '@': resolve(__dirname, 'src')
      },
      conditions: ['node'],
    },

    ssr: {
      target: 'node',
      external: ['electron'],
      noExternal: true,
    },

    plugins: [
      react(),
      ...(isMain ? [asset(), napi(), binary()] : []),
    ],
  };
});
