import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { promisify } from 'node:util';
import passkeyLinuxBinary from 'binary:keeless-passkey-linux';
import passkeyWindowsBinary from 'binary:keeless-passkey-windows';
import { app } from 'electron';
import type { PasskeyCheck, PasskeyState } from '@/types/DesktopBridge';

const execFileAsync = promisify(execFile);
const commandTimeoutMs = 60_000;

const platform = (): PasskeyState['platform'] => {
  if (process.platform === 'linux') {
    return 'linux';
  }
  if (process.platform === 'win32') {
    return 'windows';
  }
  throw new Error('Passkey support is only available on Linux and Windows.');
};

const binaryPath = () => {
  const currentPlatform = platform();
  if (app.isPackaged) {
    return join(
      process.resourcesPath,
      'bin',
      currentPlatform === 'linux' ? 'keeless-passkey-linux' : 'keeless-passkey-windows.exe',
    );
  }

  const binary = currentPlatform === 'linux' ? passkeyLinuxBinary : passkeyWindowsBinary;
  if (!binary) {
    throw new Error(`Passkey binary is unavailable for ${currentPlatform}.`);
  }
  return binary;
};

const commandError = (error: unknown) => {
  const detail =
    typeof error === 'object' && error && 'stderr' in error && typeof error.stderr === 'string'
      ? error.stderr.trim()
      : '';
  if (/cancelled|canceled|not authorized/i.test(detail)) {
    return new Error('Authentication was cancelled.');
  }
  if (detail) {
    return new Error(detail);
  }
  return error instanceof Error ? error : new Error('The passkey command failed.');
};

const execute = async (arguments_: readonly string[]) => {
  try {
    return await execFileAsync(binaryPath(), arguments_, {
      encoding: 'utf8',
      maxBuffer: 1024 * 1024,
      timeout: commandTimeoutMs,
      windowsHide: true,
    });
  } catch (error) {
    throw commandError(error);
  }
};

const isPasskeyCheck = (value: unknown): value is PasskeyCheck => {
  if (!value || typeof value !== 'object') {
    return false;
  }
  const check = value as Record<string, unknown>;
  return (
    typeof check.id === 'string' &&
    typeof check.label === 'string' &&
    (check.status === 'ok' || check.status === 'warning' || check.status === 'error') &&
    (check.detail === undefined || typeof check.detail === 'string')
  );
};

const parseState = (output: string): PasskeyState => {
  let value: unknown;
  try {
    value = JSON.parse(output);
  } catch {
    throw new Error('Passkey status command returned invalid JSON.');
  }
  if (!value || typeof value !== 'object') {
    throw new Error('Passkey status command returned an invalid state.');
  }
  const state = value as Record<string, unknown>;
  if (
    state.platform !== platform() ||
    (state.state !== 'enabled' &&
      state.state !== 'disabled' &&
      state.state !== 'degraded' &&
      state.state !== 'unsupported') ||
    typeof state.enabled !== 'boolean' ||
    !Array.isArray(state.checks) ||
    !state.checks.every(isPasskeyCheck)
  ) {
    throw new Error('Passkey status command returned an invalid state.');
  }
  return state as PasskeyState;
};

export const getPasskeyState = async (): Promise<PasskeyState> => {
  const { stdout } = await execute(['doctor', '--json']);
  return parseState(stdout);
};

const desktopPath = () => {
  const path = process.env.APPIMAGE;
  return path && existsSync(path) ? path : undefined;
};

const wait = (milliseconds: number) => new Promise(resolve => setTimeout(resolve, milliseconds));

export const setPasskeyEnabled = async (enabled: boolean): Promise<PasskeyState> => {
  const currentPlatform = platform();
  const arguments_ = [enabled ? '--enable' : '--disable'];
  if (enabled && currentPlatform === 'linux') {
    const desktop = desktopPath();
    if (desktop) {
      arguments_.push('--desktop', desktop);
    }
  }
  await execute(arguments_);

  let state = await getPasskeyState();
  if (enabled) {
    for (let attempt = 0; state.state === 'degraded' && attempt < 4; attempt += 1) {
      await wait(200);
      state = await getPasskeyState();
    }
  }
  return state;
};
