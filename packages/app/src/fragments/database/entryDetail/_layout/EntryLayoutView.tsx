import { EntryFieldValue } from '../_components/EntryFieldValue';
import { FieldPlain } from '../_components/FieldPlain';
import { ExpiryValue } from './ExpiryValue';
import { FieldDivider } from './FieldDivider';
import { getLayoutItemKey, isAmbiguousLayoutField, resolveLayoutField } from './bindings';
import type { EntryDetailResult, EntryLayout } from '@keeless/schema';

const safeUrl = (value: string) => {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : undefined;
  } catch {
    return undefined;
  }
};

export const EntryLayoutView = ({
  detail,
  layout,
}: {
  detail: EntryDetailResult;
  layout: EntryLayout;
}) => (
  <>
    {layout.items.map((item, index) => {
      const { target, control, label } = item;
      const key = getLayoutItemKey(layout.items, index);
      if (target.type === 'passwordConfirmation') {
        return null;
      }
      if (target.type === 'divider' || control.type === 'divider') {
        return <FieldDivider key={key} label={label} />;
      }
      if (target.type === 'overrideUrl') {
        const href = safeUrl(detail.overrideUrl);
        return href ? (
          <div key={key} className="space-y-1 px-4 py-3">
            <dt className="text-xs text-muted-foreground">{label}</dt>
            <dd className="min-w-0 break-words text-sm">
              <a
                className="underline underline-offset-4"
                href={href}
                target="_blank"
                rel="noreferrer"
              >
                {detail.overrideUrl}
              </a>
            </dd>
          </div>
        ) : (
          <FieldPlain key={key} name={label} value={detail.overrideUrl} />
        );
      }
      if (target.type === 'expiry') {
        return (
          <ExpiryValue
            key={key}
            label={label}
            expires={detail.expires}
            expiryTimeMs={detail.expiryTimeMs}
          />
        );
      }
      if (target.type === 'tags') {
        return <FieldPlain key={key} name={label} value={detail.tags.join(', ')} />;
      }
      if (target.type !== 'field') {
        return null;
      }
      if (isAmbiguousLayoutField(target, detail.fields)) {
        return null;
      }
      return (
        <EntryFieldValue
          key={key}
          entryId={detail.id}
          field={resolveLayoutField(target, detail.fields)}
          label={label}
          control={control}
        />
      );
    })}
  </>
);
