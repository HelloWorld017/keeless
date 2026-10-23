import { readFileSync } from 'node:fs';
import type { Configuration, Metadata } from 'electron-builder';

const { author, description, homepage, version } = JSON.parse(
  readFileSync(new URL('../../package.json', import.meta.url), 'utf8'),
) as Metadata;

export default {
  appId: 'dev.nenw.keeless',
  productName: 'keeless',
  executableName: 'keeless',
  directories: {
    output: 'release',
  },

  extraMetadata: {
    author,
    description,
    homepage,
    version,
  },

  files: [
    'dist/**',
    '!dist/main/assets/keeless-native-ui*',
    '!dist/main/assets/keeless-passkey*',
    'package.json',
  ],

  asarUnpack: ['dist/main/assets/*.node'],
  extraResources: [{ from: 'src/assets/icons', to: 'icons' }],

  linux: {
    target: ['AppImage', 'deb', 'pacman'],
    category: 'Utility;Security;',
    icon: 'src/assets/icons/icon.png',
    maintainer: 'nenw* <khi@nenw.dev>',
    extraResources: [
      {
        from: 'dist/main/assets/keeless-native-ui',
        to: 'bin/keeless-native-ui',
      },
      {
        from: 'dist/main/assets/keeless-passkey-linux',
        to: 'bin/keeless-passkey-linux',
      },
    ],
  },

  deb: {
    afterRemove: 'build/linux/post_remove.sh',
  },

  pacman: {
    afterRemove: 'build/linux/post_remove.sh',
    compression: 'zstd',
    artifactName: '${name}-${version}-${arch}.pkg.tar.zst',
    depends: [
      'alsa-lib',
      'gtk3',
      'nss',
      'libxss',
      'libxtst',
      'libnotify',
      'libappindicator-gtk3',
      'mesa',
      'libxkbcommon',
      'systemd',
    ],
  },

  win: {
    target: ['nsis'],
    icon: 'src/assets/icons/icon.ico',
    extraResources: [
      {
        from: 'dist/main/assets/keeless-native-ui.exe',
        to: 'bin/keeless-native-ui.exe',
      },
      {
        from: 'dist/main/assets/keeless-passkey-windows.exe',
        to: 'bin/keeless-passkey-windows.exe',
      },
      {
        from: 'dist/main/assets/keeless-passkey-windows.msix',
        to: 'bin/keeless-passkey-windows.msix',
      },
    ],
  },

  nsis: {
    include: 'build/windows/installer.nsh',
    oneClick: false,
    perMachine: true,
    allowElevation: true,
  },
} satisfies Configuration;
