import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from '@/components/alert-dialog';
import { Button } from '@/components/button';
import { IconChevronDown, IconLoaderCircle, IconPlus, IconSearch, IconTrash } from '@/icons';
import { Menu } from '@base-ui/react/menu';
import { useState } from 'react';
import { getEntryTitle } from './EntryItem';
import { ItemIcon } from './ItemIcon';
import type { DatabaseNodeId, EntriesResult } from '@keeless/schema';
import type { UseQueryResult } from '@tanstack/react-query';

export const EntryListHeader = ({
  title,
  result,
  creationParentId,
  movePending,
  addPending,
  templates,
  onAdd,
  onAddFromTemplate,
  searchQuery,
  onOpenSearch,
  emptyTrashPending = false,
  onEmptyTrash,
}: {
  title: string;
  result?: EntriesResult;
  creationParentId?: DatabaseNodeId;
  movePending: boolean;
  addPending: boolean;
  templates: UseQueryResult<EntriesResult>;
  onAdd: () => void;
  onAddFromTemplate: (templateEntryId: DatabaseNodeId) => void;
  searchQuery?: string;
  onOpenSearch?: (initialQuery: string) => void;
  emptyTrashPending?: boolean;
  onEmptyTrash?: () => void;
}) => {
  const [emptyTrashOpen, setEmptyTrashOpen] = useState(false);
  const entryCount = result?.entries.length ?? 0;

  return (
    <header className="flex min-h-16 items-center justify-between gap-3 px-4 py-3 xl:py-6 xl:pb-4">
      <div className="min-w-0">
        <h1 className="truncate text-xl font-semibold">{title}</h1>
        {result && (
          <span className="shrink-0 text-sm tabular-nums text-muted-foreground">
            {entryCount} {entryCount === 1 ? 'Entry' : 'Entries'}
          </span>
        )}
      </div>
      <div className="flex shrink-0 gap-2">
        {searchQuery && onOpenSearch && (
          <Button
            type="button"
            variant="outline"
            size="icon"
            aria-label="Search this list"
            onClick={() => onOpenSearch(searchQuery)}
          >
            <IconSearch />
          </Button>
        )}
        {onEmptyTrash && (
          <AlertDialog
            open={emptyTrashOpen}
            onOpenChange={open => !emptyTrashPending && setEmptyTrashOpen(open)}
          >
            <Button
              type="button"
              variant={entryCount === 0 ? 'outline' : 'destructive'}
              size="sm"
              disabled={entryCount === 0 || emptyTrashPending}
              onClick={() => setEmptyTrashOpen(true)}
            >
              {emptyTrashPending ? <IconLoaderCircle className="animate-spin" /> : <IconTrash />}
              Empty Trash
            </Button>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Empty Trash?</AlertDialogTitle>
                <AlertDialogDescription>
                  {entryCount} {entryCount === 1 ? 'entry will' : 'entries will'} be permanently
                  deleted. This cannot be undone.
                </AlertDialogDescription>
              </AlertDialogHeader>
              <AlertDialogFooter>
                <AlertDialogCancel disabled={emptyTrashPending}>Cancel</AlertDialogCancel>
                <AlertDialogAction
                  variant="destructive"
                  disabled={emptyTrashPending}
                  onClick={() => {
                    setEmptyTrashOpen(false);
                    onEmptyTrash();
                  }}
                >
                  Empty Trash
                </AlertDialogAction>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialog>
        )}
        <div className="flex shrink-0">
          {creationParentId !== undefined && (
            <>
              <Button
                type="button"
                variant="outline"
                size="icon"
                className="rounded-r-none"
                aria-label="Add entry"
                disabled={movePending || addPending}
                onClick={onAdd}
              >
                {addPending ? <IconLoaderCircle className="animate-spin" /> : <IconPlus />}
              </Button>
              <Menu.Root>
                <Menu.Trigger
                  render={
                    <Button
                      type="button"
                      variant="outline"
                      size="icon"
                      className="-ml-px rounded-l-none"
                      aria-label="Add entry from template"
                    />
                  }
                  disabled={movePending || addPending}
                >
                  <IconChevronDown />
                </Menu.Trigger>
                <Menu.Portal>
                  <Menu.Positioner align="end" sideOffset={4} className="isolate z-50">
                    <Menu.Popup className="max-h-(--available-height) min-w-52 origin-(--transform-origin) overflow-y-auto rounded-lg bg-popover/90 p-1 text-popover-foreground shadow-md ring-1 ring-foreground/10 backdrop-blur-xl duration-100 data-[side=bottom]:slide-in-from-top-2 data-[side=top]:slide-in-from-bottom-2 data-open:animate-in data-open:fade-in-0 data-open:zoom-in-95 data-closed:animate-out data-closed:fade-out-0 data-closed:zoom-out-95">
                      {templates.isPending && (
                        <Menu.Item
                          disabled
                          className="flex items-center gap-2 rounded-md px-2 py-1.5 text-sm text-muted-foreground outline-none"
                        >
                          <IconLoaderCircle className="animate-spin" />
                          Loading templates
                        </Menu.Item>
                      )}
                      {templates.isError && (
                        <Menu.Item
                          disabled
                          className="rounded-md px-2 py-1.5 text-sm text-destructive outline-none"
                        >
                          Templates could not be loaded
                        </Menu.Item>
                      )}
                      {templates.data?.entries.length === 0 && (
                        <Menu.Item
                          disabled
                          className="rounded-md px-2 py-1.5 text-sm text-muted-foreground outline-none"
                        >
                          No templates
                        </Menu.Item>
                      )}
                      {templates.data?.entries.map(template => (
                        <Menu.Item
                          key={String(template.id)}
                          className="flex cursor-default items-center gap-2 rounded-md px-2 py-1.5 text-sm outline-none data-highlighted:bg-foreground/10"
                          onClick={() => onAddFromTemplate(template.id)}
                        >
                          <ItemIcon
                            icon={template.icon}
                            fallback="entry"
                            className="size-4 shrink-0 text-muted-foreground"
                          />
                          <span className="min-w-0 truncate">{getEntryTitle(template)}</span>
                        </Menu.Item>
                      ))}
                    </Menu.Popup>
                  </Menu.Positioner>
                </Menu.Portal>
              </Menu.Root>
            </>
          )}
        </div>
      </div>
    </header>
  );
};
