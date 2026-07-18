import '@/styles/index.css';
import { App, AppFrame } from '@/fragments/App';
import { createRoot } from 'react-dom/client';

const container = document.getElementById('app');
if (!container) {
  throw new Error('App container was not found');
}

createRoot(container).render(<AppFrame><App /></AppFrame>);
