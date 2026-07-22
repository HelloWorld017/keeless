import { execFileSync } from 'node:child_process';
import { copyFile, mkdir } from 'node:fs/promises';
import { basename, resolve } from 'node:path';

const packageRoot = resolve(import.meta.dirname, '..');
const extension = process.platform === 'win32' ? '.exe' : '';
const release = process.argv.includes('--release');
const profile = release ? 'release' : 'debug';
execFileSync('cargo', ['build', '-p', 'keeless_host_desktop', ...(release ? ['--release'] : [])], {
  cwd: resolve(packageRoot, '../..'),
  stdio: 'inherit',
});
const source = resolve(
  process.env.KEELESS_DAEMON_BINARY ??
    resolve(packageRoot, `../../target/${profile}`, `keeless-daemon${extension}`),
);
const targetTriple = execFileSync('rustc', ['-vV'], { encoding: 'utf8' })
  .split('\n')
  .find(line => line.startsWith('host: '))
  ?.slice(6);

if (!targetTriple) {
  throw new Error('Could not determine the Rust host target');
}

const directory = resolve(packageRoot, 'binaries');
const destination = resolve(directory, `keeless-daemon-${targetTriple}${extension}`);
await mkdir(directory, { recursive: true });
await copyFile(source, destination);
console.info(`Staged ${basename(source)} for ${targetTriple}`);
