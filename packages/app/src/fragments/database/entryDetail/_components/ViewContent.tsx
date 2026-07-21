import { Button } from '@/components/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/collapsible';
import { IconChevronRight } from '@/icons';
import { cn } from '@/utils/css';
import { useState, type ReactNode } from 'react';
import { ExpiryValue } from '../_layout/ExpiryValue';
import { FieldDivider } from '../_layout/FieldDivider';
import { formatBytes, formatDate } from '../_utils/format';
import { getFieldName } from '../_utils/getFieldName';
import { EntryFieldValue } from './EntryFieldValue';
import { FieldCopyButton } from './FieldCopyButton';
import { FieldPlain } from './FieldPlain';
import type {
  EntryAttachmentInformation,
  EntryDetailResult,
  EntryFieldInformation,
} from '@keeless/schema';

const DetailSection = ({
  title,
  action,
  children,
}: {
  title: string;
  action?: ReactNode;
  children: ReactNode;
}) => (
  <section className="space-y-3">
    <div className="flex items-center justify-between gap-3">
      <h2 className="text-sm font-semibold">{title}</h2>
      {action}
    </div>
    {children}
  </section>
);

const Attachment = ({ attachment }: { attachment: EntryAttachmentInformation }) => (
  <div className="flex items-center justify-between gap-4 px-4 py-3 text-sm">
    <span className="min-w-0 truncate">{attachment.name || 'Untitled attachment'}</span>
    <span className="shrink-0 text-muted-foreground">
      {attachment.isProtected && 'Protected · '}
      {formatBytes(attachment.size)}
    </span>
  </div>
);

const MetadataRow = ({ label, value }: { label: string; value: ReactNode }) => (
  <div className="grid gap-1 px-4 py-3 text-sm xl:grid-cols-[10rem_1fr] xl:gap-4">
    <dt className="text-muted-foreground">{label}</dt>
    <dd className="min-w-0 break-words xl:text-right">{value}</dd>
  </div>
);

const safeUrl = (value: string) => {
  try {
    const url = new URL(value);
    return url.protocol === 'http:' || url.protocol === 'https:' ? url.href : undefined;
  } catch {
    return undefined;
  }
};

const fieldKey = (field: EntryFieldInformation) =>
  field.type === 'field' && field.fieldId !== null
    ? `field:${field.fieldId}`
    : `${field.type}:${field.order}`;

export const ViewContent = ({ detail }: { detail: EntryDetailResult }) => {
  const [internalOpen, setInternalOpen] = useState(false);
  const colorRows = [
    detail.backgroundColor && ['Background color', detail.backgroundColor],
    detail.foregroundColor && ['Foreground color', detail.foregroundColor],
  ].filter((row): row is string[] => Boolean(row));
  const fields = detail.fields.toSorted((left, right) => left.order - right.order);
  const internalFields = fields.filter(field => field.type === 'field' && field.isInternal);
  const visibleFields = fields.filter(field => field.type !== 'field' || !field.isInternal);

  const fieldValue = (field: EntryFieldInformation): ReactNode => {
    const key = fieldKey(field);
    switch (field.type) {
      case 'passwordConfirmation':
        return null;
      case 'divider':
        return <FieldDivider key={key} label={field.label} />;
      case 'overrideUrl': {
        const href = safeUrl(detail.overrideUrl);
        return href ? (
          <div key={key} className="space-y-1 px-4 py-3">
            <dt className="text-xs text-muted-foreground">{field.label}</dt>
            <dd className="flex min-w-0 items-start gap-2 text-sm">
              <a
                className="min-w-0 flex-1 break-words underline underline-offset-4"
                href={href}
                target="_blank"
                rel="noreferrer"
              >
                {detail.overrideUrl}
              </a>
              <FieldCopyButton label={field.label} value={detail.overrideUrl} />
            </dd>
          </div>
        ) : (
          <FieldPlain key={key} name={field.label} value={detail.overrideUrl} />
        );
      }
      case 'expiry':
        return (
          <ExpiryValue
            key={key}
            label={field.label}
            expires={detail.expires}
            expiryTimeMs={detail.expiryTimeMs}
          />
        );
      case 'tags':
        return <FieldPlain key={key} name={field.label} value={detail.tags.join(', ')} />;
      case 'field':
        return (
          <EntryFieldValue
            key={key}
            field={field}
            label={field.control ? field.label : getFieldName(field.kind, field.name)}
            control={field.control}
          />
        );
    }
    return null;
  };

  return (
    <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
      <DetailSection title="Fields">
        <dl className="divide-y rounded-lg border">{visibleFields.map(fieldValue)}</dl>
        {internalFields.length > 0 && (
          <Collapsible open={internalOpen} onOpenChange={setInternalOpen}>
            <CollapsibleTrigger
              render={<Button type="button" variant="ghost" className="w-full justify-start" />}
            >
              <IconChevronRight
                className={cn('transition-transform', internalOpen && 'rotate-90')}
              />
              Internal fields
              <span className="text-muted-foreground">({internalFields.length})</span>
            </CollapsibleTrigger>
            <CollapsibleContent className="pt-2">
              <dl className="divide-y rounded-lg border">{internalFields.map(fieldValue)}</dl>
            </CollapsibleContent>
          </Collapsible>
        )}
      </DetailSection>
      {detail.tags.length > 0 && (
        <DetailSection title="Tags">
          <p className="text-sm leading-6">{detail.tags.join(', ')}</p>
        </DetailSection>
      )}
      {detail.attachments.length > 0 && (
        <DetailSection title="Attachments">
          <div className="divide-y rounded-lg border">
            {detail.attachments.map((attachment, index) => (
              <Attachment key={`${attachment.name}:${index}`} attachment={attachment} />
            ))}
          </div>
        </DetailSection>
      )}
      <DetailSection title="Details">
        <dl className="divide-y rounded-lg border">
          <MetadataRow label="Created" value={formatDate(detail.creationTimeMs)} />
          <MetadataRow label="Modified" value={formatDate(detail.lastModificationTimeMs)} />
          <MetadataRow
            label="Expires"
            value={detail.expires ? formatDate(detail.expiryTimeMs) : 'Never'}
          />
          {detail.overrideUrl && <MetadataRow label="Override URL" value={detail.overrideUrl} />}
          {colorRows.map(([label, value]) => (
            <MetadataRow key={label} label={label} value={value} />
          ))}
        </dl>
      </DetailSection>
    </div>
  );
};
