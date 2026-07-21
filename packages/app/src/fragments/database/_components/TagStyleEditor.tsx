import { Button } from '@/components/button';
import { Field, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/popover';
import { IconPencil } from '@/icons';
import { useId, useState } from 'react';
import { IconPicker } from './IconPicker';
import { Tag } from './Tag';
import type { TagStyle, TagSummary } from '@keeless/schema';

const DEFAULT_STYLE: TagStyle = {
  icon: { standardId: 0, customUuid: null },
  color: '#64748b',
};

export const TagStyleEditor = ({
  tag,
  disabled,
  onSave,
}: {
  tag: TagSummary;
  disabled?: boolean;
  onSave: (style: TagStyle) => Promise<void>;
}) => {
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<TagStyle>(tag.style ?? DEFAULT_STYLE);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState(false);
  const colorId = useId();
  const colorValid = /^#[0-9a-fA-F]{6}$/.test(draft.color);

  const save = async () => {
    if (!colorValid) {
      return;
    }
    setPending(true);
    setError(false);
    try {
      await onSave(draft);
      setOpen(false);
    } catch {
      setError(true);
    } finally {
      setPending(false);
    }
  };

  return (
    <Popover
      open={open}
      onOpenChange={nextOpen => {
        if (nextOpen) {
          setDraft(tag.style ?? DEFAULT_STYLE);
          setError(false);
        }
        setOpen(nextOpen);
      }}
    >
      <PopoverTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            className="absolute top-1 right-8 z-10 text-sidebar-foreground/60 opacity-0 transition-opacity group-hover:opacity-100 group-focus-within:opacity-100"
          />
        }
        disabled={disabled}
        aria-label={`Edit ${tag.name} tag style`}
        onClick={event => event.stopPropagation()}
      >
        <IconPencil />
      </PopoverTrigger>
      <PopoverContent align="end" className="w-72 gap-4 p-4">
        <PopoverHeader>
          <PopoverTitle>Edit tag</PopoverTitle>
          <PopoverDescription>Choose the icon and accent color for this tag.</PopoverDescription>
        </PopoverHeader>
        <div className="flex justify-center rounded-lg border bg-muted/30 p-4">
          <Tag name={tag.name} tagStyle={draft} />
        </div>
        <Field>
          <FieldLabel>Icon</FieldLabel>
          <IconPicker
            value={draft.icon}
            fallback="entry"
            disabled={pending}
            onChange={icon => setDraft(current => ({ ...current, icon }))}
          />
        </Field>
        <Field>
          <FieldLabel htmlFor={colorId}>Color</FieldLabel>
          <div className="flex items-center gap-2">
            <Input
              id={colorId}
              type="color"
              value={draft.color}
              disabled={pending}
              className="size-8 shrink-0 cursor-pointer p-1"
              onChange={event => setDraft(current => ({ ...current, color: event.target.value }))}
            />
            <Input
              value={draft.color}
              disabled={pending}
              pattern="#[0-9a-fA-F]{6}"
              aria-label="Tag color hex value"
              aria-invalid={!colorValid}
              onChange={event => setDraft(current => ({ ...current, color: event.target.value }))}
            />
          </div>
        </Field>
        {!colorValid && (
          <p className="text-xs text-destructive" role="alert">
            Enter a six-digit hex color such as #3b82f6.
          </p>
        )}
        {error && colorValid && (
          <p className="text-xs text-destructive" role="alert">
            The tag style could not be saved.
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Button type="button" variant="outline" disabled={pending} onClick={() => setOpen(false)}>
            Cancel
          </Button>
          <Button type="button" disabled={pending || !colorValid} onClick={() => void save()}>
            Save
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  );
};
