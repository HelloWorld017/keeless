import { cx } from '@/utils/css';
import { FieldCopyButton } from './FieldCopyButton';

export const FieldPlain = ({ name, value }: { name: string; value: string | null }) => (
  <div className="space-y-1 px-4 py-3">
    <dt className="text-xs text-muted-foreground">{name || 'Untitled field'}</dt>
    <dd className="flex min-w-0 items-start gap-2 text-sm">
      <span
        className={cx(
          'min-w-0 flex-1 whitespace-pre-wrap break-words',
          !value && 'text-muted-foreground',
        )}
      >
        {value || 'Empty'}
      </span>
      <FieldCopyButton label={name} value={value} />
    </dd>
  </div>
);
