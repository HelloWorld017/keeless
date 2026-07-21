import {
  Combobox,
  ComboboxChip,
  ComboboxChips,
  ComboboxChipsInput,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxItem,
  ComboboxList,
  ComboboxValue,
  useComboboxAnchor,
} from '@/components/combobox';
import { useRequest } from '@/fragments/_providers/QueryProvider';
import { cn } from '@/utils/css';
import { useState } from 'react';
import { Tag } from './Tag';

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
  const [draft, setDraft] = useState('');
  const anchor = useComboboxAnchor();
  const tags = useRequest('getTags', {});
  const summaries = tags.data?.tags ?? [];
  const trimmedDraft = draft.trim();
  const available = summaries.filter(tag => !value.includes(tag.name));
  const canCreate =
    Boolean(trimmedDraft) &&
    !value.includes(trimmedDraft) &&
    !summaries.some(tag => tag.name === trimmedDraft);
  const options = [...available.map(tag => tag.name), ...(canCreate ? [trimmedDraft] : [])];

  return (
    <Combobox
      items={options}
      multiple
      value={value}
      inputValue={draft}
      disabled={disabled}
      autoHighlight
      filter={(name, query) => name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())}
      onInputValueChange={setDraft}
      onValueChange={onChange}
    >
      <ComboboxChips
        ref={anchor}
        className={cn('w-full', disabled && 'pointer-events-none opacity-50')}
      >
        <ComboboxValue>
          {(selected: string[]) => (
            <>
              {selected.map(name => {
                const summary = summaries.find(tag => tag.name === name);
                return (
                  <ComboboxChip
                    key={name}
                    showRemove={false}
                    className="h-auto max-w-full bg-transparent p-0"
                  >
                    <Tag
                      name={name}
                      tagStyle={summary?.style}
                      onRemove={
                        disabled ? undefined : () => onChange(value.filter(tag => tag !== name))
                      }
                    />
                  </ComboboxChip>
                );
              })}
              <ComboboxChipsInput
                id={id}
                disabled={disabled}
                className="h-6 min-w-24 text-sm placeholder:text-muted-foreground"
                placeholder={selected.length ? undefined : placeholder}
                aria-label={placeholder}
                autoComplete="off"
              />
            </>
          )}
        </ComboboxValue>
      </ComboboxChips>
      <ComboboxContent anchor={anchor} className="min-w-64">
        <ComboboxEmpty>{trimmedDraft ? 'No tags found.' : 'No tags available.'}</ComboboxEmpty>
        <ComboboxList>
          {(name: string) => {
            const summary = summaries.find(tag => tag.name === name);
            return (
              <ComboboxItem key={name} value={name}>
                {canCreate && name === trimmedDraft && !summary ? (
                  <>Add &quot;{name}&quot;</>
                ) : (
                  <Tag name={name} tagStyle={summary?.style} />
                )}
              </ComboboxItem>
            );
          }}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  );
};
