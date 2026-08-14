import type { WindowControlAction } from '@/types/DesktopBridge';

const controls: readonly { action: WindowControlAction; label: string; className: string }[] = [
  { action: 'minimize', label: 'Minimize window', className: 'bg-black' },
  { action: 'maximize', label: 'Maximize window', className: 'bg-black' },
  { action: 'close', label: 'Close window', className: 'bg-[#e03a34]' },
];

export const WindowBar = () => (
  <header className="relative flex h-10 items-center justify-center border-b border-[var(--border)] bg-[var(--background)] text-[var(--foreground)] select-none [-webkit-app-region:drag]">
    <span className="text-[0.6875rem] font-bold tracking-[0.16em]">KEELESS</span>
    <div className="absolute right-4 flex gap-2 [-webkit-app-region:no-drag]">
      {controls.map(({ action, label, className }) => (
        <button
          key={action}
          type="button"
          className={`size-3 rounded-full border-0 p-0 ${className} focus-visible:outline-2 focus-visible:outline-[var(--ring)] focus-visible:outline-offset-3`}
          data-action={action}
          aria-label={label}
          title={label}
          onClick={() => void window.keelessDesktop.windowControl(action)}
        />
      ))}
    </div>
  </header>
);
