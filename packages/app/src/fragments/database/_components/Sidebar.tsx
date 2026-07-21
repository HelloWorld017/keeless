import Logo from '@/assets/images/logo.png';
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from '@/components/alert-dialog';
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
import { IconList, IconPlus, IconSearch, IconTrash } from '@/icons';
import { cx } from '@/utils/css';
import { buildRoute, getRoute } from '@/utils/route';
import { useDndContext, useDroppable } from '@dnd-kit/core';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { useMemo } from 'react';
import { Link, useLocation, useRoute } from 'wouter';
import { databaseNodeKey, type RootDropData, type TrashDropData } from '../_utils/dragAndDrop';
import { GroupTree, moveGroupInHierarchy } from './GroupTree';
import { Tag } from './Tag';
import { TagStyleEditor } from './TagStyleEditor';
import type {
  AddGroupArgs,
  DatabaseNodeId,
  DeleteGroupArgs,
  GroupHierarchyResult,
  MoveGroupArgs,
  TagStyle,
  UpdateGroupArgs,
} from '@keeless/schema';

const hierarchyQueryKey = ['request', 'getGroupHierarchy', {}] as const;
const groupDeletionQueryNames = [
  'getEntries',
  'searchEntries',
  'getGroupEntries',
  'getTagEntries',
  'getTrashEntries',
  'getTags',
  'getEntryDetail',
] as const;

const AllEntriesMenuItem = ({
  rootGroupId,
  location,
  onNavigate,
}: {
  rootGroupId: DatabaseNodeId | undefined;
  location: string;
  onNavigate: () => void;
}) => {
  const hasRootGroup = rootGroupId !== undefined;
  const { active } = useDndContext();
  const data: RootDropData | undefined = hasRootGroup
    ? { type: 'root', groupId: rootGroupId, title: 'All Entries' }
    : undefined;
  const { isOver, setNodeRef } = useDroppable({
    id: hasRootGroup ? `root:${databaseNodeKey(rootGroupId)}` : 'root:unavailable',
    data,
    disabled: !hasRootGroup || active?.data.current?.type === 'group',
  });
  const isEntryOver = isOver && active?.data.current?.type === 'entry';

  return (
    <SidebarMenuItem ref={setNodeRef}>
      <SidebarMenuButton
        render={<Link href={buildRoute('database')} />}
        isActive={location === buildRoute('database') || isEntryOver}
        className={cx('border-2 border-transparent', isEntryOver && 'border-sidebar-ring')}
        onClick={onNavigate}
      >
        <IconList />
        <span>All Entries</span>
      </SidebarMenuButton>
    </SidebarMenuItem>
  );
};

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

const DatabaseSidebar = ({ onSearch }: { onSearch: () => void }) => {
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

  const updateGroup = useMutation({
    mutationFn: async (args: UpdateGroupArgs) => {
      await requestClient.data!.request('updateGroup', args);
    },
    onMutate: async args => {
      await queryClient.cancelQueries({ queryKey: hierarchyQueryKey });
      const previous = queryClient.getQueryData<GroupHierarchyResult>(hierarchyQueryKey);
      queryClient.setQueryData<GroupHierarchyResult>(hierarchyQueryKey, current =>
        current
          ? {
              ...current,
              groups: current.groups.map(group =>
                group.id === args.groupId ? { ...group, name: args.name, icon: args.icon } : group,
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
  const updateTagStyle = useMutation({
    mutationFn: async ({ name, style }: { name: string; style: TagStyle }) => {
      await requestClient.data!.request('updateTagStyle', { name, style });
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: ['request', 'getTags'] }),
  });
  const deleteTag = useMutation({
    mutationFn: async (name: string) => {
      await requestClient.data!.request('deleteTag', { name });
    },
    onSuccess: async (_result, name) => {
      const href = buildRoute('tag', { tag: name });
      if (location === href) {
        setLocation(buildRoute('database'), { replace: true });
      }
      await queryClient.invalidateQueries({ queryKey: ['request', 'getTags'] });
    },
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
    moveGroup.isPending || addGroup.isPending || updateGroup.isPending || deleteGroup.isPending;

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
              <AllEntriesMenuItem
                rootGroupId={hierarchy.data?.rootGroupId}
                location={location}
                onNavigate={closeMobile}
              />
              <SidebarMenuItem className="border-2 border-transparent">
                <SidebarMenuButton
                  onClick={() => {
                    closeMobile();
                    onSearch();
                  }}
                >
                  <IconSearch />
                  <span>Search</span>
                  <kbd className="ml-auto text-[10px] text-sidebar-foreground/60">^P</kbd>
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
                onUpdate={async args => {
                  await updateGroup.mutateAsync(args);
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
            {updateGroup.isError && (
              <p className="px-2 pt-2 text-xs text-destructive" role="alert">
                The group could not be updated.
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
                    <SidebarMenuItem key={tag.name} className="group">
                      <SidebarMenuButton
                        render={<Link href={href} replace />}
                        isActive={location === href}
                        className="pr-16"
                        onClick={closeMobile}
                      >
                        <Tag name={tag.name} style={tag.style} compact className="max-w-36" />
                      </SidebarMenuButton>
                      <TagStyleEditor
                        tag={tag}
                        disabled={updateTagStyle.isPending || deleteTag.isPending}
                        onSave={style => updateTagStyle.mutateAsync({ name: tag.name, style })}
                      />
                      {tag.canDelete && (
                        <AlertDialog>
                          <AlertDialogTrigger
                            render={
                              <Button
                                type="button"
                                variant="ghost"
                                size="icon-xs"
                                className="absolute top-1 right-14 z-10 text-destructive opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
                                aria-label={`Delete ${tag.name} tag`}
                              />
                            }
                            disabled={updateTagStyle.isPending || deleteTag.isPending}
                            onClick={event => event.stopPropagation()}
                          >
                            <IconTrash />
                          </AlertDialogTrigger>
                          <AlertDialogContent>
                            <AlertDialogHeader>
                              <AlertDialogTitle>Delete tag?</AlertDialogTitle>
                              <AlertDialogDescription>
                                The unused "{tag.name}" tag and its style will be removed.
                              </AlertDialogDescription>
                            </AlertDialogHeader>
                            <AlertDialogFooter>
                              <AlertDialogCancel>Cancel</AlertDialogCancel>
                              <AlertDialogAction
                                variant="destructive"
                                onClick={() => deleteTag.mutate(tag.name)}
                              >
                                Delete
                              </AlertDialogAction>
                            </AlertDialogFooter>
                          </AlertDialogContent>
                        </AlertDialog>
                      )}
                      <SidebarMenuBadge>{tag.entryCount}</SidebarMenuBadge>
                    </SidebarMenuItem>
                  );
                })}
              </SidebarMenu>
            )}
            {(updateTagStyle.isError || deleteTag.isError) && (
              <p className="px-2 pt-2 text-xs text-destructive" role="alert">
                The tag could not be updated.
              </p>
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
