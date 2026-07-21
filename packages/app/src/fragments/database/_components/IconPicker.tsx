import { Button } from '@/components/button';
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from '@/components/command';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/popover';
import { useRequest } from '@/fragments/_providers/QueryProvider';
import { IconCheck } from '@/icons';
import { cn } from '@/utils/css';
import { useState } from 'react';
import { ItemIcon } from './ItemIcon';
import { standardIcons } from '../_constants/icons';
import type { IconReference } from '@keeless/schema';
import type { ComponentProps } from 'react';

type IconPickerProps = {
  value: IconReference;
  iconClassName?: string;
  onChange: (value: IconReference) => void;
  disabled?: boolean;
  fallback: 'entry' | 'group';
  render?: ComponentProps<typeof PopoverTrigger>['render'];
};

const sameIcon = (left: IconReference, right: IconReference) =>
  left.standardId === right.standardId &&
  (left.customUuid ?? '').toLowerCase() === (right.customUuid ?? '').toLowerCase();

export const IconPicker = ({ value, iconClassName, onChange, disabled, fallback, render }: IconPickerProps) => {
  const [open, setOpen] = useState(false);
  const customIcons = useRequest('getCustomIcons', {});
  const select = (icon: IconReference) => {
    onChange(icon);
    setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={render ?? <Button type="button" variant="outline" size="icon" />}
        disabled={disabled}
        aria-label="Choose icon"
      >
        <ItemIcon className={iconClassName} icon={value} fallback={fallback} />
      </PopoverTrigger>
      <PopoverContent className="w-80 p-0" align="start">
        <Command>
          <CommandInput placeholder="Search icons..." />
          <CommandList>
            <CommandEmpty>No icons found.</CommandEmpty>
            <CommandGroup heading="Standard icons" className="grid grid-cols-6">
              {standardIcons.map(({ id, label, searchTerms, Icon }) => {
                const icon = { standardId: id, customUuid: null };
                const selected = sameIcon(value, icon);
                return (
                  <CommandItem
                    key={id}
                    value={`${label} ${searchTerms}`}
                    className="relative aspect-square justify-center px-0 py-0"
                    aria-label={label}
                    title={label}
                    onSelect={() => select(icon)}
                  >
                    <Icon className="size-5" />
                    <IconCheck
                      className={cn(
                        'absolute right-0.5 bottom-0.5 size-3 rounded-full bg-primary p-0.5 text-primary-foreground',
                        !selected && 'hidden',
                      )}
                    />
                    <span className="sr-only">{selected ? 'Selected' : ''}</span>
                  </CommandItem>
                );
              })}
            </CommandGroup>
            {(customIcons.data?.icons.length ?? 0) > 0 && (
              <CommandGroup heading="Custom icons" className="grid grid-cols-6">
                {customIcons.data!.icons.map(custom => {
                  const icon = { standardId: value.standardId, customUuid: custom.uuid };
                  const selected = sameIcon(value, icon);
                  return (
                    <CommandItem
                      key={custom.uuid}
                      value={`${custom.name} ${custom.uuid}`}
                      className="relative aspect-square justify-center px-0 py-0"
                      aria-label={custom.name || 'Custom icon'}
                      title={custom.name || 'Custom icon'}
                      onSelect={() => select(icon)}
                    >
                      <img
                        src={`data:image/png;base64,${custom.dataBase64}`}
                        alt=""
                        className="size-5 object-contain"
                      />
                      <IconCheck
                        className={cn(
                          'absolute right-0.5 bottom-0.5 size-3 rounded-full bg-primary p-0.5 text-primary-foreground',
                          !selected && 'hidden',
                        )}
                      />
                      <span className="sr-only">{selected ? 'Selected' : ''}</span>
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
};
