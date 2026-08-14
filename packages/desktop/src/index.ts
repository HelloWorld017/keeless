import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import iconIco from '@/assets/icons/icon.ico?asset';
import iconPng from '@/assets/icons/icon.png?asset';
import keeless from '@keeless/host-desktop';
import nativeUiPath from 'binary:keeless-native-ui';
import { app, BrowserWindow, dialog, ipcMain, Menu, nativeImage, shell, Tray } from 'electron';
import type { DesktopHost as DesktopHostType } from '@keeless/host-desktop';
import type { MessageFrame } from '@keeless/lesswire';

const dirname = fileURLToPath(new URL('.', import.meta.url));
const minimized = process.argv.slice(1).includes('--minimized');
const hasLock = app.requestSingleInstanceLock();

let browserWindow: BrowserWindow | undefined;
let tray: Tray | undefined;
let host: DesktopHostType | undefined;
let quitting = false;
let shutdownComplete = false;
let registrationCompromised = false;
let clientRegistered = false;

const iconPath = () => (__PLATFORM__ === 'win32' ? iconIco : iconPng);

const databaseFilters = [{ name: 'KeePass database', extensions: ['kdbx'] }];

const showWindow = () => {
  if (!browserWindow) {
    return;
  }
  if (browserWindow.isMinimized()) {
    browserWindow.restore();
  }
  browserWindow.show();
  browserWindow.focus();
};

const assertSender = (event: Electron.IpcMainInvokeEvent) => {
  if (!browserWindow || event.sender !== browserWindow.webContents) {
    throw new Error('Untrusted desktop IPC sender');
  }
  if (registrationCompromised) {
    throw new Error('Desktop client registration is disabled');
  }
};

const terminateForRepeatedClientRegistration = () => {
  registrationCompromised = true;
  void dialog
    .showMessageBox({
      type: 'error',
      title: 'Security error',
      message: 'Client registration was attempted more than once.',
      detail: 'Keeless will now exit.',
    })
    .finally(() => app.exit(1));
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
    if (typeof bundle !== 'string') {
      throw new Error('Invalid client bundle');
    }
    if (!host) {
      throw new Error('Desktop host is unavailable');
    }
    if (clientRegistered) {
      terminateForRepeatedClientRegistration();
      throw new Error('Client registration was attempted more than once');
    }
    const recipient = await host.addRuntimeClient(bundle);
    clientRegistered = true;
    return recipient;
  });

  ipcMain.handle('desktop:relay-frame', async (event, frame: unknown) => {
    assertSender(event);
    const bytes = Buffer.from(JSON.stringify(frame));
    const response = await host?.handleFrame(bytes);
    return response ? (JSON.parse(response.toString('utf8')) as MessageFrame) : null;
  });

  ipcMain.handle('desktop:pick-local-file', async (event, mode: unknown) => {
    assertSender(event);
    if (mode !== 'open' && mode !== 'create') {
      throw new Error('Invalid file picker mode');
    }
    if (!browserWindow || !host) {
      throw new Error('Desktop host is unavailable');
    }

    const path =
      mode === 'open'
        ? await dialog
            .showOpenDialog(browserWindow, {
              title: 'Choose a KeePass database',
              filters: databaseFilters,
              properties: ['openFile'],
            })
            .then(result => result.filePaths[0])
        : await dialog
            .showSaveDialog(browserWindow, {
              title: 'Choose a KeePass database',
              filters: databaseFilters,
            })
            .then(result => result.filePath);

    return path ? host.grantLocalFile(path) : null;
  });

  ipcMain.handle('desktop:window-control', (event, action: unknown) => {
    assertSender(event);
    if (!browserWindow) {
      throw new Error('Desktop window is unavailable');
    }

    switch (action) {
      case 'minimize':
        browserWindow.minimize();
        return;
      case 'maximize':
        if (browserWindow.isMaximized()) {
          browserWindow.unmaximize();
        } else {
          browserWindow.maximize();
        }
        return;
      case 'close':
        browserWindow.close();
        return;
      default:
        throw new Error('Invalid window control action');
    }
  });
};

const createWindow = async () => {
  clientRegistered = false;
  browserWindow = new BrowserWindow({
    title: 'keeless',
    width: 1440,
    height: 900,
    minWidth: 720,
    minHeight: 560,
    show: false,
    frame: false,
    autoHideMenuBar: true,
    icon: iconPath(),
    webPreferences: {
      preload: join(dirname, '../preload/index.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true,
    },
  });
  browserWindow.removeMenu();

  browserWindow.on('close', event => {
    if (!quitting) {
      event.preventDefault();
      browserWindow?.hide();
    }
  });

  browserWindow.webContents.setWindowOpenHandler(({ url }) => {
    if (url.startsWith('https://')) {
      void shell.openExternal(url);
    }
    return { action: 'deny' };
  });

  browserWindow.webContents.on('will-navigate', event => event.preventDefault());
  browserWindow.setContentProtection(true);

  if (process.env.ELECTRON_RENDERER_URL) {
    await waitForOnline(process.env.ELECTRON_RENDERER_URL);
    await browserWindow.loadURL(process.env.ELECTRON_RENDERER_URL);
  } else {
    await browserWindow.loadFile(join(dirname, '../renderer/index.html'));
  }

  if (!minimized) {
    showWindow();
  }
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
  ipcMain.removeHandler('desktop:window-control');
  await host?.shutdown();
  host = undefined;
  tray?.destroy();
  tray = undefined;
};

if (!hasLock) {
  app.quit();
} else {
  app.on('second-instance', (_event, commandLine) => {
    if (!commandLine.includes('--minimized')) {
      showWindow();
    }
  });
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
    Menu.setApplicationMenu(null);
    host = await keeless.DesktopHost.create(nativeUiPath);
    host.onEntryFocus(entryId => {
      showWindow();
      browserWindow?.webContents.send('desktop:focus-entry', entryId);
    });
    installIpc();
    createTray();
    await createWindow();
  });
}
