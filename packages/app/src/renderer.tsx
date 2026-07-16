import { App } from '@/fragments/App';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

const container = document.getElementById('app');
if (!container) {
  throw new Error('App container was not found');
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
