import { useEffect, useState } from 'react';

const MOBILE_BREAKPOINT = 768;
const DESKTOP_BREAKPOINT = 1280;

export function useIsMobile() {
  const [isMobile, setIsMobile] = useState(false);

  useEffect(() => {
    const mediaQuery = window.matchMedia(`(max-width: ${MOBILE_BREAKPOINT - 1}px)`);
    const update = () => setIsMobile(window.innerWidth < MOBILE_BREAKPOINT);
    mediaQuery.addEventListener('change', update);
    update();
    return () => mediaQuery.removeEventListener('change', update);
  }, []);

  return isMobile;
}

export function useIsDesktop() {
  const [isDesktop, setIsDesktop] = useState(false);

  useEffect(() => {
    const mediaQuery = window.matchMedia(`(min-width: ${DESKTOP_BREAKPOINT}px)`);
    const update = () => setIsDesktop(window.innerWidth >= DESKTOP_BREAKPOINT);
    mediaQuery.addEventListener('change', update);
    update();
    return () => mediaQuery.removeEventListener('change', update);
  }, []);

  return isDesktop;
}
