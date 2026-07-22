import { createRoot } from 'react-dom/client';
import { App } from './index';

const container = document.getElementById('app');
if (!container) {
  throw new Error('App container was not found');
}

createRoot(container).render(<App integration={{}} />);
