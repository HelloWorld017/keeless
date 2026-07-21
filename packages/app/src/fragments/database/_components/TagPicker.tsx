import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandItem,
  CommandList,
} from '@/components/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/popover';
import { useRequest } from '@/fragments/_providers/QueryProvider';
import { IconCheck } from '@/icons';
import { cn } from '@/utils/css';
import { useId, useRef, useState } from 'react';
import { Tag } from './Tag';
import type { KeyboardEvent } from 'react';

type TagPickerProps = {
  value: string[];
  onChange: (value: string[]) => void;
  disabled?: boolean;
  id?: string;
  placeholder?: string;
};

export const TagPicker = ({
  value,
  onChange,
  disabled,
  id,
  placeholder = 'Add a tag...',
}: TagPickerProps) => {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState('');
  const [activeIndex, setActiveIndex] = useState<number | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const generatedId = useId();
  const inputId = id ?? generatedId;
  const listId = `${inputId}-suggestions`;
  const tags = useRequest('getTags', {});
  const trimmedDraft = draft.trim();
  const available = (tags.data?.tags ?? []).filter(
    tag =>
      !value.includes(tag.name) &&
      (!trimmedDraft || tag.name.toLocaleLowerCase().includes(trimmedDraft.toLocaleLowerCase())),
  );
  const canCreate =
    Boolean(trimmedDraft) &&
    !value.includes(trimmedDraft) &&
    !available.some(tag => tag.name === trimmedDraft);
  const optionCount = available.length + (canCreate ? 1 : 0);

  const add = (name: string) => {
    const trimmed = name.trim();
    if (!trimmed || value.includes(trimmed)) {
      return;
    }
    onChange([...value, trimmed]);
    setDraft('');
    setActiveIndex(null);
    setOpen(true);
    requestAnimationFrame(() => inputRef.current?.focus());
  };
  const remove = (name: string) => onChange(value.filter(tag => tag !== name));
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Backspace' && !draft && value.length > 0) {
      event.preventDefault();
      onChange(value.slice(0, -1));
    }
    if ((event.key === 'ArrowDown' || event.key === 'ArrowUp') && optionCount > 0) {
      event.preventDefault();
      setOpen(true);
      setActiveIndex(current => {
        if (current === null) {
          return event.key === 'ArrowDown' ? 0 : optionCount - 1;
        }
        return (current + (event.key === 'ArrowDown' ? 1 : -1) + optionCount) % optionCount;
      });
      return;
    }
    if (event.key === 'Tab' && open && trimmedDraft && optionCount > 0) {
      event.preventDefault();
      setActiveIndex(current =>
        current === null ? 0 : (current + (event.shiftKey ? -1 : 1) + optionCount) % optionCount,
      );
      return;
    }
    if (event.key === 'Enter' && (trimmedDraft || activeIndex !== null)) {
      event.preventDefault();
      if (activeIndex !== null && activeIndex < available.length) {
        add(available[activeIndex].name);
      } else {
        add(trimmedDraft);
      }
    }
  };

  return (
    <label
      htmlFor={inputId}
      className={cn(
        'flex min-h-8 w-full flex-wrap items-center gap-1 rounded-lg border border-input bg-transparent px-2 py-1 transition-colors focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50',
        disabled && 'pointer-events-none opacity-50',
      )}
    >
      {value.map(name => {
        const summary = tags.data?.tags.find(tag => tag.name === name);
        return (
          <Tag
            key={name}
            name={name}
            style={summary?.style}
            compact
            onRemove={() => remove(name)}
          />
        );
      })}
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <input
              ref={inputRef}
              id={inputId}
              value={draft}
              disabled={disabled}
              className="h-6 min-w-24 flex-1 bg-transparent text-sm outline-none placeholder:text-muted-foreground"
              placeholder={value.length ? undefined : placeholder}
              aria-label={placeholder}
              role="combobox"
              aria-autocomplete="list"
              aria-expanded={open}
              aria-controls={listId}
              aria-activedescendant={
                activeIndex === null ? undefined : `${listId}-option-${activeIndex}`
              }
              autoComplete="off"
              onChange={event => {
                setDraft(event.target.value);
                setActiveIndex(null);
                setOpen(true);
              }}
              onFocus={() => setOpen(true)}
              onKeyDown={onKeyDown}
            />
          }
        />
        <PopoverContent
          className="w-(--anchor-width) min-w-64 p-0"
          align="start"
          onMouseDown={event => event.preventDefault()}
        >
          <Command shouldFilter={false}>
            <CommandList id={listId}>
              {available.length === 0 && !trimmedDraft && (
                <CommandEmpty>No tags available.</CommandEmpty>
              )}
              {available.length > 0 && (
                <CommandGroup heading="Tags">
                  {available.map((tag, index) => (
                    <CommandItem
                      key={tag.name}
                      id={`${listId}-option-${index}`}
                      value={tag.name}
                      className={cn(activeIndex === index && 'bg-muted')}
                      onMouseMove={() => setActiveIndex(index)}
                      onSelect={() => add(tag.name)}
                    >
                      <Tag name={tag.name} style={tag.style} compact />
                      <IconCheck className="ml-auto size-4 opacity-0" />
                    </CommandItem>
                  ))}
                </CommandGroup>
              )}
              {canCreate && (
                <CommandGroup heading="Create">
                  <CommandItem
                    id={`${listId}-option-${available.length}`}
                    value={`create:${trimmedDraft}`}
                    className={cn(activeIndex === available.length && 'bg-muted')}
                    onMouseMove={() => setActiveIndex(available.length)}
                    onSelect={() => add(trimmedDraft)}
                  >
                    Add &quot;{trimmedDraft}&quot;
                  </CommandItem>
                </CommandGroup>
              )}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>
    </label>
  );
};
