import Logo from '@/assets/images/logo.png';
import {
  Sidebar,
  SidebarContent,
  SidebarEmpty,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuBadge,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarMenuSkeleton,
  SidebarSeparator,
  useSidebar,
} from '@/components/sidebar';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { IconDatabase, IconList, IconTag, IconTrash } from '@/icons';
import { buildRoute } from '@/utils/route';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { Link, useLocation } from 'wouter';
import { DatabaseGroupTree, moveGroupInHierarchy } from './DatabaseGroupTree';
import type { GroupHierarchyResult, MoveGroupArgs } from '@keeless/schema';
import {useMemo} from 'react';

const hierarchyQueryKey = ['request', 'getGroupHierarchy', {}] as const;

export const DatabaseSidebar = () => {
  const [location] = useLocation();
  const hierarchy = useRequest('getGroupHierarchy', {});
  const tags = useRequest('getTags', {});
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const { isMobile, setOpenMobile } = useSidebar();
  const closeMobile = () => {
    if (isMobile) {
      setOpenMobile(false);
    }
  };
  const moveGroup = useMutation({
    mutationFn: (args: MoveGroupArgs) => requestClient.data!.request('moveGroup', args),
    onMutate: async args => {
      await queryClient.cancelQueries({ queryKey: hierarchyQueryKey });
      const previous = queryClient.getQueryData<GroupHierarchyResult>(hierarchyQueryKey);
      queryClient.setQueryData<GroupHierarchyResult>(hierarchyQueryKey, current =>
        current ? moveGroupInHierarchy(current, args) : current,
      );
      return { previous };
    },
    onError: (_error, _args, context) => {
      if (context?.previous) {
        queryClient.setQueryData(hierarchyQueryKey, context.previous);
      }
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: hierarchyQueryKey }),
  });
  const databaseName = hierarchy.data
    ? hierarchy.data.databaseName ||
      hierarchy.data.groups.find(group => group.id === hierarchy.data.rootGroupId)?.name ||
      'Database'
    : undefined;

  const storage = useRequest('getStorageDescriptor', {});
  const storageName = useMemo(() => {
    const provider = storage.data?.storage?.provider;
    const storages = requestClient.data?.host.storages;
    return storages?.find(storage => storage.kind === provider)?.label;
  }, [requestClient.data, storage.data]);

  return (
    <Sidebar>
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" disabled={!databaseName}>
              <div className='flex gap-4 items-center'>
                <div className="aspect-square size-8">
                  <img src={Logo} />
                </div>
                <div className='flex flex-col'>
                  <span className="font-semibold">{databaseName ?? 'Loading database'}</span>
                  <span>{storageName}</span>
                </div>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarSeparator />
      <SidebarContent>
        <SidebarGroup>
          <SidebarGroupContent>
            <SidebarMenu>
              <SidebarMenuItem>
                <SidebarMenuButton
                  render={<Link href={buildRoute('database')} />}
                  isActive={location === buildRoute('database')}
                  onClick={closeMobile}
                >
                  <IconList />
                  <span>All Entries</span>
                </SidebarMenuButton>
              </SidebarMenuItem>
            </SidebarMenu>
          </SidebarGroupContent>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Groups</SidebarGroupLabel>
          <SidebarGroupContent>
            {hierarchy.isPending && <SidebarMenuSkeleton />}
            {hierarchy.isError && <SidebarEmpty>Groups could not be loaded.</SidebarEmpty>}
            {hierarchy.data && (
              <DatabaseGroupTree
                hierarchy={hierarchy.data}
                location={location}
                disabled={moveGroup.isPending}
                onMove={args => moveGroup.mutate(args)}
                onNavigate={closeMobile}
              />
            )}
            {moveGroup.isError && (
              <p className="px-2 pt-2 text-xs text-destructive" role="alert">
                The group could not be moved.
              </p>
            )}
          </SidebarGroupContent>
        </SidebarGroup>
        <SidebarGroup>
          <SidebarGroupLabel>Tags</SidebarGroupLabel>
          <SidebarGroupContent>
            {tags.isPending && <SidebarMenuSkeleton />}
            {tags.isError && <SidebarEmpty>Tags could not be loaded.</SidebarEmpty>}
            {tags.data && tags.data.tags.length === 0 && <SidebarEmpty>No tags</SidebarEmpty>}
            {tags.data && tags.data.tags.length > 0 && (
              <SidebarMenu>
                {tags.data.tags.map(tag => {
                  const href = buildRoute('tag', { tag: tag.name });
                  return (
                    <SidebarMenuItem key={tag.name}>
                      <SidebarMenuButton
                        render={<Link href={href} />}
                        isActive={location === href}
                        className="pr-8"
                        onClick={closeMobile}
                      >
                        <IconTag />
                        <span>{tag.name}</span>
                      </SidebarMenuButton>
                      <SidebarMenuBadge>{tag.entryCount}</SidebarMenuBadge>
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            )}
          </SidebarGroupContent>
        </SidebarGroup>
      </SidebarContent>
      <SidebarSeparator />
      <SidebarFooter>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton
              render={<Link href={buildRoute('trash')} />}
              isActive={location === buildRoute('trash')}
              onClick={closeMobile}
            >
              <IconTrash />
              <span>Trash</span>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
};
