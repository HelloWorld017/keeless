import { Input } from '@/components/input';
import { cn } from '@/utils/css';

export const FieldPlainEditor = ({
  id,
  name,
  value,
  placeholder,
  multiline,
  disabled,
  onChange,
}: {
  id?: string;
  name: string;
  value: string;
  placeholder?: string;
  multiline?: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
}) =>
  multiline ? (
    <textarea
      id={id}
      value={value}
      rows={4}
      className={cn(
        'min-h-24 w-full min-w-0 resize-y rounded-lg border border-input bg-transparent px-2.5 py-2 text-base outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:bg-input/50 disabled:opacity-50 md:text-sm dark:bg-input/30 dark:disabled:bg-input/80',
      )}
      placeholder={placeholder}
      aria-label={`${name} value`}
      disabled={disabled}
      onChange={event => onChange(event.target.value)}
    />
  ) : (
    <Input
      id={id}
      value={value}
      placeholder={placeholder}
      aria-label={`${name} value`}
      disabled={disabled}
      onChange={event => onChange(event.target.value)}
    />
  );
