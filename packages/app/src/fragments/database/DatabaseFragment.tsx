import { SidebarInset, SidebarProvider, SidebarTrigger } from '@/components/sidebar';
import { DatabaseSidebar } from './DatabaseSidebar';

export const DatabaseFragment = () => (
  <SidebarProvider>
    <DatabaseSidebar />
    <SidebarInset>
      <header className="flex h-12 shrink-0 items-center border-b px-3">
        <SidebarTrigger />
      </header>
    </SidebarInset>
  </SidebarProvider>
);
