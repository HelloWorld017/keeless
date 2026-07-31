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
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/dropdown-menu';
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
import { useHasNativePasswordInput } from '@/fragments/_providers/HostProvider';
import {
  useRequest,
  useRequestClient,
  useRequestMutation,
} from '@/fragments/_providers/QueryProvider';
import { useShowToast } from '@/fragments/_providers/ToastProvider';
import { PasswordPrompt } from '@/fragments/database/entryDetail/_components/PasswordPrompt';
import {
  IconList,
  IconLoaderCircle,
  IconLockKeyhole,
  IconPlus,
  IconRefreshCw,
  IconSearch,
  IconSettings,
  IconTrash,
} from '@/icons';
import { cx } from '@/utils/css';
import { CoreRequestError, queryKey } from '@/utils/request';
import { buildRoute, getRoute } from '@/utils/route';
import { useDndContext, useDroppable } from '@dnd-kit/core';
import { useQueryClient } from '@tanstack/react-query';
import { useMemo, useState } from 'react';
import { Link, useLocation, useRoute } from 'wouter';
import { databaseNodeKey, type RootDropData, type TrashDropData } from '../_utils/dragAndDrop';
import { GroupTree, moveGroupInHierarchy } from './GroupTree';
import { Tag } from './Tag';
import { TagStyleEditor } from './TagStyleEditor';
import type { DatabaseNodeId, GroupHierarchyResult } from '@keeless/schema';

const hierarchyQueryKey = queryKey('getGroupHierarchy', {});

type PasswordRequest = {
  action: 'lock' | 'sync';
  invalid: boolean;
};

const needsPassword = (error: unknown) =>
  error instanceof CoreRequestError &&
  (error.code === 'password_required' || error.code === 'invalid_credentials');

const operationError = (error: unknown, fallback: string) =>
  error instanceof Error && error.message ? error.message : fallback;
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
          'border-2 border-transparent -mx-[2px]',
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

type DatabaseSidebarProps = {
  onSearch: (initialQuery?: string) => void;
  onConfigOpen: () => void;
};

const DatabaseSidebar = ({ onSearch, onConfigOpen }: DatabaseSidebarProps) => {
  const [location, setLocation] = useLocation();
  const [groupMatch, groupParams] = useRoute<{ group: string }>(getRoute('group'));
  const hierarchy = useRequest('getGroupHierarchy', {});
  const tags = useRequest('getTags', {});
  const databaseStatus = useRequest('getDatabaseStatus', {});
  const requestClient = useRequestClient();
  const queryClient = useQueryClient();
  const hasNativePasswordInput = useHasNativePasswordInput();
  const showToast = useShowToast();
  const [passwordRequest, setPasswordRequest] = useState<PasswordRequest>();
  const { isMobile, setOpenMobile } = useSidebar();
  const closeMobile = () => {
    if (isMobile) {
      setOpenMobile(false);
    }
  };

  const moveGroup = useRequestMutation('moveGroup', {
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
  });

  const addGroup = useRequestMutation('addGroup', {
    onSuccess: result => {
      setLocation(buildRoute('group', { group: String(result.id) }));
      closeMobile();
    },
  });

  const updateGroup = useRequestMutation('updateGroup', {
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
  });
  const updateTagStyle = useRequestMutation('updateTagStyle');
  const deleteTag = useRequestMutation('deleteTag', {
    onSuccess: (_result, { name }) => {
      const href = buildRoute('tag', { tag: name });
      if (location === href) {
        setLocation(buildRoute('database'), { replace: true });
      }
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
  const deleteGroup = useRequestMutation('deleteGroup', {
    onSuccess: () => {
      const destination =
        activeGroupParent &&
        hierarchy.data &&
        databaseNodeKey(activeGroupParent.id) !== databaseNodeKey(hierarchy.data.rootGroupId)
          ? buildRoute('group', { group: String(activeGroupParent.id) })
          : buildRoute('database');
      setLocation(destination, { replace: true });
      closeMobile();
    },
  });
  const syncDatabase = useRequestMutation('saveDatabase');
  const lockDatabase = useRequestMutation('lock');
  const runDatabaseAction = async (action: PasswordRequest['action'], password?: string) => {
    try {
      if (action === 'sync') {
        await syncDatabase.mutateAsync(password === undefined ? {} : { password });
      } else {
        await lockDatabase.mutateAsync(password === undefined ? {} : { password });
        closeMobile();
      }
      setPasswordRequest(undefined);
    } catch (error) {
      if (!hasNativePasswordInput && needsPassword(error)) {
        setPasswordRequest({
          action,
          invalid: error instanceof CoreRequestError && error.code === 'invalid_credentials',
        });
        return;
      }
      showToast({
        kind: 'destructive',
        message: operationError(
          error,
          action === 'sync'
            ? 'The database could not be synchronized.'
            : 'The database could not be locked.',
        ),
      });
    }
  };
  const groupParentId = activeGroup?.id ?? hierarchy.data?.rootGroupId;
  const groupsPending =
    moveGroup.isPending || addGroup.isPending || updateGroup.isPending || deleteGroup.isPending;
  const databaseActionPending = syncDatabase.isPending || lockDatabase.isPending;

  return (
    <Sidebar className="p-2 xl:p-4">
      <SidebarHeader>
        <SidebarMenu>
          <SidebarMenuItem>
            <DropdownMenu>
              <DropdownMenuTrigger
                render={<SidebarMenuButton size="lg" disabled={!databaseName} />}
              >
                <div className="flex gap-3 items-center w-full">
                  <div className="relative aspect-square size-8">
                    <img src={Logo} alt="" />
                    {databaseStatus.data?.dirty && (
                      <span
                        className="absolute -top-0.5 -right-0.5 size-2 rounded-full bg-amber-500 ring-2 ring-sidebar"
                        role="status"
                        aria-label="Database has unsynchronized changes"
                      />
                    )}
                  </div>
                  <div className="flex flex-[1_1_0] min-w-0 flex-col">
                    <span className="font-semibold truncate">
                      {databaseName ?? 'Loading database'}
                    </span>
                    <span>{storageName}</span>
                  </div>
                </div>
              </DropdownMenuTrigger>
              <DropdownMenuContent className="max-w-40">
                <DropdownMenuItem
                  disabled={databaseActionPending}
                  onClick={() => void runDatabaseAction('sync')}
                >
                  {syncDatabase.isPending ? (
                    <IconLoaderCircle className="animate-spin" />
                  ) : (
                    <IconRefreshCw />
                  )}
                  Sync
                </DropdownMenuItem>
                <DropdownMenuItem
                  disabled={databaseActionPending}
                  onClick={() => void runDatabaseAction('lock')}
                >
                  <IconLockKeyhole />
                  Lock
                </DropdownMenuItem>
              </DropdownMenuContent>
            </DropdownMenu>
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
                        <Tag name={tag.name} tagStyle={tag.style} className="max-w-36" />
                      </SidebarMenuButton>
                      <TagStyleEditor
                        tag={tag}
                        disabled={updateTagStyle.isPending || deleteTag.isPending}
                        onSave={async style => {
                          await updateTagStyle.mutateAsync({ name: tag.name, style });
                        }}
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
                                onClick={() => deleteTag.mutate({ name: tag.name })}
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
          <SidebarMenuItem>
            <SidebarMenuButton onClick={onConfigOpen}>
              <IconSettings />
              Settings
            </SidebarMenuButton>
          </SidebarMenuItem>
        </SidebarMenu>
      </SidebarFooter>
      <PasswordPrompt
        open={passwordRequest !== undefined}
        pending={databaseActionPending}
        error={passwordRequest?.invalid ? 'Incorrect master password.' : undefined}
        title={passwordRequest?.action === 'sync' ? 'Sync database' : 'Lock database'}
        description="Enter the master password to synchronize the database before continuing."
        action={passwordRequest?.action === 'sync' ? 'Sync' : 'Lock'}
        onOpenChange={open => {
          if (!open && !databaseActionPending) {
            setPasswordRequest(undefined);
          }
        }}
        onSubmit={password => {
          if (passwordRequest) {
            void runDatabaseAction(passwordRequest.action, password);
          }
        }}
      />
    </Sidebar>
  );
};

export { DatabaseSidebar as Sidebar };
