import { spawnSync } from 'node:child_process';
import process from 'node:process';
import { fileURLToPath } from 'node:url';

if (process.platform !== 'win32') {
  process.exit(0);
}

const development = process.argv.includes('--development');
const script = fileURLToPath(new URL('./generate-msix.ps1', import.meta.url));
const result = spawnSync(
  'powershell.exe',
  [
    '-NoLogo',
    '-NoProfile',
    '-NonInteractive',
    '-ExecutionPolicy',
    'Bypass',
    '-File',
    script,
    ...(development ? ['-Development'] : []),
  ],
  { stdio: 'inherit' },
);

process.exit(result.status ?? 1);
