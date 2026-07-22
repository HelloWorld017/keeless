import { App } from '@keeless/app';
import '@keeless/app/styles.css';
import { createRoot } from 'react-dom/client';
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

createRoot(root).render(<App integration={integration} />);
