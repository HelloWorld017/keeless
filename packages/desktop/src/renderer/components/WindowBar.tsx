import { cx } from '@/utils/css';
import type { WindowControlAction } from '@/types/DesktopBridge';

const controls: readonly { action: WindowControlAction; label: string; className?: string }[] = [
  { action: 'minimize', label: 'Minimize window' },
  { action: 'maximize', label: 'Maximize window' },
  { action: 'close', label: 'Close window', className: 'bg-destructive' },
];

export const WindowBar = () => (
  <header className="relative flex h-10 items-center justify-center border-b border-border bg-background text-foreground select-none [-webkit-app-region:drag]">
    <span className="text-[0.6875rem] font-bold tracking-[0.16em]">KEELESS</span>
    <div className="absolute right-4 flex gap-3 [-webkit-app-region:no-drag]">
      {controls.map(({ action, label, className }) => (
        <button
          key={action}
          type="button"
          className={cx(
            'size-2.75 rounded-full border-0 p-0 focus-visible:outline-2 focus-visible:outline-ring focus-visible:outline-offset-3 hover:scale-110 transition bg-foreground',
            className,
          )}
          data-action={action}
          aria-label={label}
          title={label}
          onClick={() => void window.keelessDesktop.windowControl(action)}
        />
      ))}
    </div>
  </header>
);
