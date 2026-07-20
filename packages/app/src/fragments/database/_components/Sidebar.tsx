import Logo from '@/assets/images/logo.png';
import { Button } from '@/components/button';
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
import { IconList, IconPlus, IconTag, IconTrash } from '@/icons';
import { cx } from '@/utils/css';
import { buildRoute, getRoute } from '@/utils/route';
import { useDndContext, useDroppable } from '@dnd-kit/core';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useMemo } from 'react';
import { Link, useLocation, useRoute } from 'wouter';
import { databaseNodeKey, type TrashDropData } from '../_utils/dragAndDrop';
import { GroupTree, moveGroupInHierarchy } from './GroupTree';
import type {
  AddGroupArgs,
  DatabaseNodeId,
  DeleteGroupArgs,
  GroupHierarchyResult,
  MoveGroupArgs,
  RenameGroupArgs,
} from '@keeless/schema';

const hierarchyQueryKey = ['request', 'getGroupHierarchy', {}] as const;
const groupDeletionQueryNames = [
  'getEntries',
  'getGroupEntries',
  'getTagEntries',
  'getTrashEntries',
  'getTags',
  'getEntryDetail',
] as const;

const TrashMenuItem = ({
  recycleBinId,
  location,
  onNavigate,
}: {
  recycleBinId: DatabaseNodeId | null | undefined;
  location: string;
  onNavigate: () => void;
}) => {
  const hasRecycleBin = recycleBinId !== null && recycleBinId !== undefined;
  const { active } = useDndContext();
  const data: TrashDropData | undefined = hasRecycleBin
    ? { type: 'trash', groupId: recycleBinId, title: 'Trash' }
    : undefined;
  const { isOver, setNodeRef } = useDroppable({
    id: hasRecycleBin ? `trash:${databaseNodeKey(recycleBinId)}` : 'trash:unavailable',
    data,
    disabled: !hasRecycleBin || active?.data.current?.type === 'group',
  });
  const isEntryOver = isOver && active?.data.current?.type === 'entry';

  return (
    <SidebarMenuItem ref={setNodeRef}>
      <SidebarMenuButton
        render={<Link href={buildRoute('trash')} replace />}
        isActive={location === buildRoute('trash') || isEntryOver}
        className={cx(
          'border-2 border-transparent',
          isEntryOver && 'border-destructive/50 bg-destructive/25!',
        )}
        onClick={onNavigate}
      >
        <IconTrash className={cx(isEntryOver && 'text-destructive')} />
        <span className={cx(isEntryOver && 'text-destructive')}>Trash</span>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
};

const DatabaseSidebar = () => {
  const [location, setLocation] = useLocation();
  const [groupMatch, groupParams] = useRoute<{ group: string }>(getRoute('group'));
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
      const cancellation = queryClient.cancelQueries({ queryKey: hierarchyQueryKey });
      const previous = queryClient.getQueryData<GroupHierarchyResult>(hierarchyQueryKey);
      queryClient.setQueryData<GroupHierarchyResult>(hierarchyQueryKey, current =>
        current ? moveGroupInHierarchy(current, args) : current,
      );
      await cancellation;
      return { previous };
    },
    onError: (_error, _args, context) => {
      if (context?.previous) {
        queryClient.setQueryData(hierarchyQueryKey, context.previous);
      }
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: hierarchyQueryKey }),
  });

  const addGroup = useMutation({
    mutationFn: (args: AddGroupArgs) => requestClient.data!.request('addGroup', args),
    onSuccess: async result => {
      await queryClient.invalidateQueries({ queryKey: hierarchyQueryKey });
      setLocation(buildRoute('group', { group: String(result.id) }));
      closeMobile();
    },
  });

  const renameGroup = useMutation({
    mutationFn: (args: RenameGroupArgs) => requestClient.data!.request('renameGroup', args),
    onMutate: async args => {
      await queryClient.cancelQueries({ queryKey: hierarchyQueryKey });
      const previous = queryClient.getQueryData<GroupHierarchyResult>(hierarchyQueryKey);
      queryClient.setQueryData<GroupHierarchyResult>(hierarchyQueryKey, current =>
        current
          ? {
              ...current,
              groups: current.groups.map(group =>
                group.id === args.groupId ? { ...group, name: args.name } : group,
              ),
            }
          : current,
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
    return storages?.find(candidate => candidate.kind === provider)?.label;
  }, [requestClient.data, storage.data]);
  const activeGroup =
    groupMatch && hierarchy.data
      ? hierarchy.data.groups.find(group => String(group.id) === groupParams.group)
      : undefined;
  const activeGroupParent =
    activeGroup && hierarchy.data
      ? hierarchy.data.groups.find(group =>
          group.childGroupIds.some(id => databaseNodeKey(id) === databaseNodeKey(activeGroup.id)),
        )
      : undefined;
  const deleteGroup = useMutation({
    mutationFn: (args: DeleteGroupArgs) => requestClient.data!.request('deleteGroup', args),
    onSuccess: async () => {
      const destination =
        activeGroupParent &&
        hierarchy.data &&
        databaseNodeKey(activeGroupParent.id) !== databaseNodeKey(hierarchy.data.rootGroupId)
          ? buildRoute('group', { group: String(activeGroupParent.id) })
          : buildRoute('database');
      setLocation(destination, { replace: true });
      closeMobile();
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: hierarchyQueryKey }),
        ...groupDeletionQueryNames.map(name =>
          queryClient.invalidateQueries({ queryKey: ['request', name] }),
        ),
      ]);
    },
  });
  const groupParentId = activeGroup?.id ?? hierarchy.data?.rootGroupId;
  const groupsPending =
    moveGroup.isPending || addGroup.isPending || renameGroup.isPending || deleteGroup.isPending;

  return (
    <Sidebar className="p-2 xl:p-4">
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <SidebarMenuButton size="lg" disabled={!databaseName}>
              <div className="flex gap-3 items-center w-full">
                <div className="aspect-square size-8">
                  <img src={Logo} alt="" />
                </div>
                <div className="flex flex-[1_1_0] min-w-0 flex-col">
                  <span className="font-semibold truncate">
                    {databaseName ?? 'Loading database'}
                  </span>
                  <span>{storageName}</span>
                </div>
              </div>
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarHeader>
      <SidebarContent className="mt-4">
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
          <SidebarGroupLabel className="justify-between">
            <span>Groups</span>
            <Button
              type="button"
              variant="ghost"
              size="icon-xs"
              aria-label="Add group"
              disabled={!groupParentId || groupsPending}
              onClick={() => groupParentId && addGroup.mutate({ parentGroupId: groupParentId })}
            >
              <IconPlus />
            </Button>
          </SidebarGroupLabel>
          <SidebarGroupContent>
            {hierarchy.isPending && <SidebarMenuSkeleton />}
            {hierarchy.isError && <SidebarEmpty>Groups could not be loaded.</SidebarEmpty>}
            {hierarchy.data && (
              <GroupTree
                hierarchy={hierarchy.data}
                location={location}
                disabled={groupsPending}
                onMove={args => moveGroup.mutate(args)}
                onRename={async args => {
                  await renameGroup.mutateAsync(args);
                }}
                onDelete={() => {
                  if (activeGroup) {
                    deleteGroup.mutate({ groupId: activeGroup.id });
                  }
                }}
                onNavigate={closeMobile}
              />
            )}
            {moveGroup.isError && (
              <p className="px-2 pt-2 text-xs text-destructive" role="alert">
                The group could not be moved.
              </p>
            )}
            {addGroup.isError && (
              <p className="px-2 pt-2 text-xs text-destructive" role="alert">
                The group could not be added.
              </p>
            )}
            {renameGroup.isError && (
              <p className="px-2 pt-2 text-xs text-destructive" role="alert">
                The group could not be renamed.
              </p>
            )}
            {deleteGroup.isError && (
              <p className="px-2 pt-2 text-xs text-destructive" role="alert">
                The group could not be moved to Trash.
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
                        render={<Link href={href} replace />}
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
          <TrashMenuItem
            recycleBinId={hierarchy.data?.recycleBinId}
            location={location}
            onNavigate={closeMobile}
          />
        </SidebarMenu>
      </SidebarFooter>
    </Sidebar>
  );
};

export { DatabaseSidebar as Sidebar };
