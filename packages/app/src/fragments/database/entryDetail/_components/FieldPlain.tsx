import { cn } from '@/utils/css';

export const FieldPlain = ({ name, value }: { name: string; value: string | null }) => (
  <div className="space-y-1 px-4 py-3">
    <dt className="text-xs text-muted-foreground">{name || 'Untitled field'}</dt>
    <dd
      className={cn(
        'min-w-0 whitespace-pre-wrap break-words text-sm',
        !value && 'text-muted-foreground',
      )}
    >
      {value || 'Empty'}
    </dd>
  </div>
);
