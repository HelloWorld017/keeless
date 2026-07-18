import { App } from '@/fragments/App';
import { QueryProvider, RouterProvider } from '@/fragments/_providers';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import '@/index.css';

const container = document.getElementById('app');
if (!container) {
  throw new Error('App container was not found');
}

createRoot(container).render(
  <StrictMode>
    <QueryProvider>
      <RouterProvider fallback="open">
        <App />
      </RouterProvider>
    </QueryProvider>
  </StrictMode>,
);
