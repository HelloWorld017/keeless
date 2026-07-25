import type { DesktopBridge } from '@/types/DesktopBridge';

declare global {
  interface Window {
    keelessDesktop: DesktopBridge;
  }

  const __PLATFORM__: (typeof process)['platform'];
}

export {};
