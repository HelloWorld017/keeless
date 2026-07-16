import { defineConfig } from '@simplei18n/core';

export default defineConfig({
  target: {
    include: ['./src/**/*.tsx'],
    outDir: './src/i18n',
    eager: true,
  },
  locales: ['en', 'ko'],
  defaultLocale: 'ko',
});
