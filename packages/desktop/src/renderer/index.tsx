import '@keeless/app/styles.css';
import './styles.css';
import { App } from '@keeless/app';
import { createRoot } from 'react-dom/client';
import { WindowBar } from './components/WindowBar';
import { desktopHost } from './host';
import type { AppIntegration } from '@keeless/app';

const integration: AppIntegration = {
  hostOverride: desktopHost,
  hasNativePasswordInput: true,
};

const root = document.getElementById('root');

if (!root) {
  throw new Error('Desktop root element is missing');
}

createRoot(root).render(
  <div className="grid h-dvh grid-rows-[auto_1fr] overflow-hidden">
    <WindowBar />
    <main className="min-h-0 overflow-hidden [&_.h-dvh]:h-full [&_.h-svh]:h-full [&_.min-h-svh]:min-h-full">
      <App integration={integration} />
    </main>
  </div>,
);
