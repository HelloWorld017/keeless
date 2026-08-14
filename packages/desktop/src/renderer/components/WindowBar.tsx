import './WindowBar.css';
import type { WindowControlAction } from '@/types/DesktopBridge';

const controls: readonly { action: WindowControlAction; label: string }[] = [
  { action: 'minimize', label: 'Minimize window' },
  { action: 'maximize', label: 'Maximize window' },
  { action: 'close', label: 'Close window' },
];

export const WindowBar = () => (
  <header className="window-bar">
    <span className="window-bar__title">KEELESS</span>
    <div className="window-bar__controls">
      {controls.map(({ action, label }) => (
        <button
          key={action}
          type="button"
          className="window-bar__control"
          data-action={action}
          aria-label={label}
          title={label}
          onClick={() => void window.keelessDesktop.windowControl(action)}
        />
      ))}
    </div>
  </header>
);
