import { Button } from '@/components/button';
import { Separator } from '@/components/separator';
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/sheet';
import { Skeleton } from '@/components/skeleton';
import { useIsDesktop, useIsMobile } from '@/hooks/useIsMobile';
import { IconChevronLeft, IconPanelLeft } from '@/icons';
import { cn } from '@/utils/css';
import { mergeProps } from '@base-ui/react/merge-props';
import { useRender } from '@base-ui/react/use-render';
import { cva, type VariantProps } from 'class-variance-authority';
import { createContext, useCallback, useContext, useEffect, useMemo, useState } from 'react';
import type { ComponentProps, CSSProperties, ReactNode } from 'react';

const SIDEBAR_COOKIE_NAME = 'sidebar_state';
const SIDEBAR_COOKIE_MAX_AGE = 60 * 60 * 24 * 7;
const SIDEBAR_WIDTH = '16rem';
const SIDEBAR_WIDTH_ICON = '4rem';
const SIDEBAR_WIDTH_MOBILE = '18rem';
const SIDEBAR_KEYBOARD_SHORTCUT = 'b';

type SidebarContextValue = {
  open: boolean;
  setOpen: (open: boolean) => void;
  openMobile: boolean;
  setOpenMobile: (open: boolean) => void;
  isMobile: boolean;
  isDesktop: boolean;
  compactOpen: boolean;
  setCompactOpen: (open: boolean) => void;
  isCollapsed: boolean;
  expandSidebar: () => void;
  collapseSidebar: () => void;
  toggleSidebar: () => void;
};

const SidebarContext = createContext<SidebarContextValue | null>(null);

function useSidebar() {
  const context = useContext(SidebarContext);
  if (!context) {
    throw new Error('useSidebar must be used within a SidebarProvider.');
  }
  return context;
}

function SidebarProvider({
  defaultOpen = true,
  open: openProp,
  onOpenChange,
  className,
  style,
  children,
  ...props
}: ComponentProps<'div'> & {
  defaultOpen?: boolean;
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
}) {
  const isMobile = useIsMobile();
  const isDesktop = useIsDesktop();
  const [openMobile, setOpenMobile] = useState(false);
  const [compactOpen, setCompactOpen] = useState(false);
  const [internalOpen, setInternalOpen] = useState(defaultOpen);
  const open = openProp ?? internalOpen;
  const isCollapsed = !isMobile && !(isDesktop ? open : compactOpen);
  const setOpen = useCallback(
    (value: boolean) => {
      onOpenChange?.(value);
      if (!onOpenChange) {
        setInternalOpen(value);
      }
      document.cookie = `${SIDEBAR_COOKIE_NAME}=${value}; path=/; max-age=${SIDEBAR_COOKIE_MAX_AGE}`;
    },
    [onOpenChange],
  );
  const toggleSidebar = useCallback(() => {
    if (isMobile) {
      setOpenMobile(value => !value);
    } else if (!isDesktop) {
      setCompactOpen(value => !value);
    } else {
      setOpen(!open);
    }
  }, [isDesktop, isMobile, open, setOpen]);
  const expandSidebar = useCallback(() => {
    if (isMobile) {
      setOpenMobile(true);
    } else if (isDesktop) {
      setOpen(true);
    } else {
      setCompactOpen(true);
    }
  }, [isDesktop, isMobile, setOpen]);
  const collapseSidebar = useCallback(() => {
    if (isMobile) {
      setOpenMobile(false);
    } else if (isDesktop) {
      setOpen(false);
    } else {
      setCompactOpen(false);
    }
  }, [isDesktop, isMobile, setOpen]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === SIDEBAR_KEYBOARD_SHORTCUT && (event.metaKey || event.ctrlKey)) {
        event.preventDefault();
        toggleSidebar();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [toggleSidebar]);

  const value = useMemo(
    () => ({
      open,
      setOpen,
      openMobile,
      setOpenMobile,
      isMobile,
      isDesktop,
      compactOpen,
      setCompactOpen,
      isCollapsed,
      expandSidebar,
      collapseSidebar,
      toggleSidebar,
    }),
    [
      collapseSidebar,
      compactOpen,
      expandSidebar,
      isCollapsed,
      isDesktop,
      isMobile,
      open,
      openMobile,
      setOpen,
      toggleSidebar,
    ],
  );

  return (
    <SidebarContext.Provider value={value}>
      <div
        data-slot="sidebar-wrapper"
        style={
          {
            '--sidebar-width': SIDEBAR_WIDTH,
            '--sidebar-width-icon': SIDEBAR_WIDTH_ICON,
            ...style,
          } as CSSProperties
        }
        className={cn('group/sidebar-wrapper flex h-full w-full bg-background', className)}
        {...props}
      >
        {children}
      </div>
    </SidebarContext.Provider>
  );
}

function Sidebar({ className, children, ...props }: ComponentProps<'div'>) {
  const { isMobile, isDesktop, isCollapsed, openMobile, setOpenMobile } = useSidebar();
  if (isMobile) {
    return (
      <Sheet open={openMobile} onOpenChange={setOpenMobile}>
        <SheetContent
          data-sidebar="sidebar"
          data-slot="sidebar"
          side="left"
          showCloseButton={false}
          className="w-(--sidebar-width) bg-sidebar p-0 text-sidebar-foreground"
          style={{ '--sidebar-width': SIDEBAR_WIDTH_MOBILE } as CSSProperties}
        >
          <SheetHeader className="sr-only">
            <SheetTitle>Sidebar</SheetTitle>
            <SheetDescription>Database navigation</SheetDescription>
          </SheetHeader>
          <div className="flex h-full w-full flex-col">{children}</div>
        </SheetContent>
      </Sheet>
    );
  }

  return (
    <div
      data-slot="sidebar"
      data-sidebar="sidebar"
      data-state={isCollapsed ? 'collapsed' : 'expanded'}
      data-mode={isDesktop ? 'desktop' : 'compact'}
      className={cn(
        'relative hidden h-full shrink-0 overflow-hidden border-r border-sidebar-border bg-sidebar text-sidebar-foreground transition-[width] duration-200 md:flex',
        isCollapsed ? 'w-(--sidebar-width-icon)' : 'w-(--sidebar-width)',
      )}
      {...props}
    >
      <div className={cn('flex h-full w-full min-w-0 flex-col', className)}>{children}</div>
    </div>
  );
}

function SidebarExpandTrigger({ className, onClick, ...props }: ComponentProps<typeof Button>) {
  const { expandSidebar, isCollapsed, isMobile } = useSidebar();
  return (
    <Button
      data-slot="sidebar-expand-trigger"
      variant="ghost"
      size="icon-sm"
      className={cn(
        'shrink-0 overflow-hidden transition-[width,margin,opacity] duration-200',
        !isMobile && !isCollapsed && '-mr-4 w-0 opacity-0 pointer-events-none',
        className,
      )}
      aria-label="Expand Sidebar"
      onClick={event => {
        onClick?.(event);
        expandSidebar();
      }}
      {...props}
    >
      <IconPanelLeft />
      <span className="sr-only">Expand Sidebar</span>
    </Button>
  );
}

function SidebarCollapseTrigger({ className, onClick, ...props }: ComponentProps<typeof Button>) {
  const { collapseSidebar, isCollapsed } = useSidebar();
  return (
    <Button
      data-slot="sidebar-collapse-trigger"
      variant="ghost"
      size="icon-sm"
      className={cn(
        'shrink-0 overflow-hidden transition-[width,opacity] duration-200',
        isCollapsed && 'w-0 opacity-0 pointer-events-none',
        className,
      )}
      aria-label="Collapse Sidebar"
      onClick={event => {
        onClick?.(event);
        collapseSidebar();
      }}
      {...props}
    >
      <IconChevronLeft />
      <span className="sr-only">Collapse Sidebar</span>
    </Button>
  );
}

function SidebarInset({ className, ...props }: ComponentProps<'main'>) {
  return (
    <main
      data-slot="sidebar-inset"
      className={cn('relative flex min-w-0 flex-1 flex-col bg-background', className)}
      {...props}
    />
  );
}

function SidebarHeader({ className, ...props }: ComponentProps<'div'>) {
  const { isCollapsed } = useSidebar();
  return (
    <div
      data-slot="sidebar-header"
      className={cn(
        'flex flex-col gap-2 p-2 transition-[padding] duration-200',
        isCollapsed && 'p-0!',
        className,
      )}
      {...props}
    />
  );
}

function SidebarFooter({ className, ...props }: ComponentProps<'div'>) {
  const { isCollapsed } = useSidebar();
  return (
    <div
      data-slot="sidebar-footer"
      className={cn(
        'flex flex-col gap-2 p-2 transition-[padding] duration-200',
        isCollapsed && 'p-0!',
        className,
      )}
      {...props}
    />
  );
}

function SidebarContent({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="sidebar-content"
      className={cn('flex min-h-0 flex-1 flex-col overflow-auto', className)}
      {...props}
    />
  );
}

function SidebarSeparator({ className, ...props }: ComponentProps<typeof Separator>) {
  return (
    <Separator
      data-slot="sidebar-separator"
      className={cn('mx-2 w-auto bg-sidebar-border', className)}
      {...props}
    />
  );
}

function SidebarGroup({ className, ...props }: ComponentProps<'div'>) {
  const { isCollapsed } = useSidebar();
  return (
    <div
      data-slot="sidebar-group"
      className={cn(
        'relative flex w-full min-w-0 flex-col p-2 transition-[padding] duration-200',
        isCollapsed && 'p-0!',
        className,
      )}
      {...props}
    />
  );
}

function SidebarGroupLabel({ className, ...props }: ComponentProps<'div'>) {
  const { isCollapsed } = useSidebar();
  return (
    <div
      data-slot="sidebar-group-label"
      className={cn(
        'flex h-8 shrink-0 items-center overflow-hidden px-2 text-xs font-medium text-sidebar-foreground/70 transition-[height,opacity] duration-200',
        isCollapsed && 'h-0 opacity-0',
        className,
      )}
      {...props}
    />
  );
}

function SidebarGroupContent({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div data-slot="sidebar-group-content" className={cn('w-full text-sm', className)} {...props} />
  );
}

function SidebarMenu({ className, ...props }: ComponentProps<'ul'>) {
  return (
    <ul
      data-slot="sidebar-menu"
      className={cn('flex w-full min-w-0 flex-col', className)}
      {...props}
    />
  );
}

function SidebarMenuItem({ className, ...props }: ComponentProps<'li'>) {
  return (
    <li
      data-slot="sidebar-menu-item"
      className={cn('group/menu-item relative', className)}
      {...props}
    />
  );
}

const sidebarMenuButtonVariants = cva(
  'peer/menu-button relative flex w-full items-center gap-2 overflow-hidden rounded-md p-2 text-left text-sm outline-hidden transition-[background-color,color,padding] duration-200 hover:bg-sidebar-accent hover:text-sidebar-accent-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring disabled:pointer-events-none disabled:opacity-50 data-active:bg-sidebar-accent data-active:font-medium data-active:text-sidebar-accent-foreground [&_svg]:size-4 [&_svg]:shrink-0 [&_[data-sidebar-label]]:transition-opacity [&_[data-sidebar-label]]:duration-200 [&>span:last-child]:truncate',
  {
    variants: {
      size: { default: 'h-8', sm: 'h-7 text-xs', lg: 'h-12' },
    },
    defaultVariants: { size: 'default' },
  },
);

function SidebarMenuButton({
  render,
  isActive = false,
  size = 'default',
  className,
  ...props
}: useRender.ComponentProps<'button'> &
  ComponentProps<'button'> & { isActive?: boolean } & VariantProps<
    typeof sidebarMenuButtonVariants
  >) {
  const { isCollapsed } = useSidebar();
  return useRender({
    defaultTagName: 'button',
    props: mergeProps<'button'>(
      {
        className: cn(
          sidebarMenuButtonVariants({ size }),
          isCollapsed && [
            size === 'lg'
              ? 'px-[calc((var(--sidebar-width-icon)_-_2rem)_/_2)]'
              : 'px-[calc((var(--sidebar-width-icon)_-_1rem)_/_2)]',
            '[&>[data-sidebar-label]]:opacity-0 [&>kbd]:opacity-0',
          ],
          className,
        ),
      },
      props,
    ),
    render,
    state: { slot: 'sidebar-menu-button', active: isActive, size },
  });
}

function SidebarMenuBadge({ className, ...props }: ComponentProps<'div'>) {
  const { isCollapsed } = useSidebar();
  return (
    <div
      data-slot="sidebar-menu-badge"
      className={cn(
        'pointer-events-none absolute top-1.5 right-1 flex h-5 min-w-5 items-center justify-center rounded-md px-1 text-xs tabular-nums transition-opacity duration-200',
        isCollapsed && 'opacity-0',
        className,
      )}
      {...props}
    />
  );
}

function SidebarMenuSkeleton({ className, ...props }: ComponentProps<'div'>) {
  return (
    <div
      data-slot="sidebar-menu-skeleton"
      className={cn('flex h-8 items-center gap-2 rounded-md px-2', className)}
      {...props}
    >
      <Skeleton className="size-4" />
      <Skeleton className="h-4 flex-1" />
    </div>
  );
}

function SidebarEmpty({
  children,
  hideWhenCollapsed = false,
}: {
  children: ReactNode;
  hideWhenCollapsed?: boolean;
}) {
  const { isCollapsed } = useSidebar();
  return (
    <p
      className={cn(
        'max-h-20 overflow-hidden px-2 py-1.5 text-xs text-sidebar-foreground/60 transition-[max-height,opacity,padding] duration-200',
        hideWhenCollapsed && isCollapsed && 'max-h-0 py-0 opacity-0',
      )}
    >
      {children}
    </p>
  );
}

export {
  Sidebar,
  SidebarCollapseTrigger,
  SidebarContent,
  SidebarEmpty,
  SidebarExpandTrigger,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarInset,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
  SidebarProvider,
  SidebarSeparator,
  useSidebar,
};
