import '@keeless/app/styles.css';
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
  <div className="desktop-shell">
    <WindowBar />
    <main className="desktop-app">
      <App integration={integration} />
    </main>
  </div>,
);
