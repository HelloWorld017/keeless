import { Input } from '@/components/input';
import { cn } from '@/utils/css';

export const FieldPlainEditor = ({
  name,
  value,
  multiline,
  disabled,
  onChange,
}: {
  name: string;
  value: string;
  multiline?: boolean;
  disabled: boolean;
  onChange: (value: string) => void;
}) =>
  multiline ? (
    <textarea
      value={value}
      rows={5}
      className={cn(
        'w-full min-w-0 resize-y rounded-lg border border-input bg-transparent px-2.5 py-2 text-base outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 md:text-sm dark:bg-input/30',
      )}
      aria-label={`${name} value`}
      disabled={disabled}
      onChange={event => onChange(event.target.value)}
    />
  ) : (
    <Input
      value={value}
      aria-label={`${name} value`}
      disabled={disabled}
      onChange={event => onChange(event.target.value)}
    />
  );
