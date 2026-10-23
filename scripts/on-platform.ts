#!/usr/bin/env node
import { spawnSync } from 'node:child_process';

const [platform, ...scripts] = process.argv.slice(2);
const platforms: NodeJS.Platform[] = [
  'aix',
  'android',
  'darwin',
  'freebsd',
  'haiku',
  'linux',
  'openbsd',
  'sunos',
  'win32',
  'cygwin',
  'netbsd',
];

if (!platforms.includes(platform as NodeJS.Platform) || scripts.length === 0) {
  console.error('Usage: on-platform <platform> <script> [script...]');
  process.exit(1);
}

if (process.platform !== platform) {
  console.info(
    `Skipping ${scripts.join(', ')}: requires ${platform} (current: ${process.platform})`,
  );
  process.exit(0);
}

const packageManager = process.env.npm_execpath;
if (!packageManager) {
  console.error('on-platform must be run through a package manager script');
  process.exit(1);
}

for (const script of scripts) {
  const result = spawnSync(process.execPath, [packageManager, 'run', script], {
    stdio: 'inherit',
  });

  if (result.error) {
    console.error(result.error);
    process.exit(1);
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}
