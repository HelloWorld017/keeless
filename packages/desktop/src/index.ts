import { app, BrowserWindow, ipcMain, Menu, nativeImage, shell, Tray } from 'electron';
import keeless from '@keeless/host-desktop';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import iconIco from '@/assets/icons/icon.ico?asset';
import iconPng from '@/assets/icons/icon.png?asset';
import nativeUiPath from 'binary:keeless-native-ui';
import type { MessageFrame } from '@keeless/lesswire';
import type { DesktopHost as DesktopHostType } from '@keeless/host-desktop';

const dirname = fileURLToPath(new URL('.', import.meta.url));
const minimized = process.argv.slice(1).includes('--minimized');
const hasLock = app.requestSingleInstanceLock();

let browserWindow: BrowserWindow | undefined;
let tray: Tray | undefined;
let host: DesktopHostType | undefined;
let quitting = false;
let shutdownComplete = false;

const iconPath = () =>
  __PLATFORM__ === 'win32' ? iconIco : iconPng;

const showWindow = () => {
  if (!browserWindow) return;
  if (browserWindow.isMinimized()) browserWindow.restore();
  browserWindow.show();
  browserWindow.focus();
};

const assertSender = (event: Electron.IpcMainInvokeEvent) => {
  if (!browserWindow || event.sender !== browserWindow.webContents) {
    throw new Error('Untrusted desktop IPC sender');
  }
};

const waitForOnline = (url: string) => {
  const wait = (retry = 0): Promise<unknown> =>
    fetch(url).catch(async err => {
      if (retry >= 5) {
        throw err;
      }

      await new Promise(resolve => setTimeout(resolve, 100 * 2 ** retry));
      return wait(retry + 1);
    });

  return wait();
};

const installIpc = () => {
  ipcMain.handle('desktop:register-client', async (event, bundle: unknown) => {
    assertSender(event);
    if (typeof bundle !== 'string') throw new Error('Invalid client bundle');
    await host?.registerClient(bundle);
  });

  ipcMain.handle('desktop:relay-frame', async (event, frame: unknown) => {
    assertSender(event);
    const bytes = Buffer.from(JSON.stringify(frame));
    const response = await host?.handleFrame(bytes);
    return response ? (JSON.parse(response.toString('utf8')) as MessageFrame) : null;
  });

  ipcMain.handle('desktop:pick-local-file', async (event, mode: unknown) => {
    assertSender(event);
    if (mode !== 'open' && mode !== 'create') throw new Error('Invalid file picker mode');
    return host?.pickLocalFile(mode);
  });
};

const createWindow = async () => {
  browserWindow = new BrowserWindow({
    title: 'keeless',
    width: 1440,
    height: 900,
    minWidth: 720,
    minHeight: 560,
    show: false,
    icon: iconPath(),
    webPreferences: {
      preload: join(dirname, '../preload/index.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });

  browserWindow.on('close', event => {
    if (!quitting) {
      event.preventDefault();
      browserWindow?.hide();
    }
  });

  browserWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith('https://')) void shell.openExternal(url);
    return { action: 'deny' };
  });

  browserWindow.webContents.on('will-navigate', event => event.preventDefault());

  if (process.env.ELECTRON_RENDERER_URL) {
    await waitForOnline(process.env.ELECTRON_RENDERER_URL);
    await browserWindow.loadURL(process.env.ELECTRON_RENDERER_URL);
  } else {
    await browserWindow.loadFile(join(dirname, '../renderer/index.html'));
  }

  if (!minimized) showWindow();
};

const createTray = () => {
  const image = nativeImage.createFromPath(iconPath());
  tray = new Tray(image);
  tray.setToolTip('keeless');
  tray.setContextMenu(
    Menu.buildFromTemplate([
      { label: 'Open', click: showWindow },
      { type: 'separator' },
      { label: 'Quit', click: () => app.quit() },
    ]),
  );
  tray.on('double-click', showWindow);
};

const shutdown = async () => {
  ipcMain.removeHandler('desktop:register-client');
  ipcMain.removeHandler('desktop:relay-frame');
  ipcMain.removeHandler('desktop:pick-local-file');
  await host?.shutdown();
  host = undefined;
  tray?.destroy();
  tray = undefined;
};

if (!hasLock) {
  app.quit();
} else {
  app.on('second-instance', showWindow);
  app.on('before-quit', event => {
    quitting = true;
    if (!shutdownComplete) {
      event.preventDefault();
      void shutdown().finally(() => {
        shutdownComplete = true;
        app.quit();
      });
    }
  });
  app.on('window-all-closed', () => {});
  app.on('activate', showWindow);
  void app.whenReady().then(async () => {
    host = await keeless.DesktopHost.create(nativeUiPath);
    installIpc();
    createTray();
    await createWindow();
  });
}
