import { EntryLayoutView } from '../_layout/EntryLayoutView';
import { resolveLayoutField } from '../_layout/bindings';
import { formatBytes, formatDate } from '../_utils/format';
import { getFieldName } from '../_utils/getFieldName';
import { EntryFieldValue } from './EntryFieldValue';
import type { EntryAttachmentInformation, EntryDetailResult } from '@keeless/schema';
import type { ReactNode } from 'react';

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

export const ViewContent = ({ detail }: { detail: EntryDetailResult }) => {
  const colorRows = [
    detail.backgroundColor && ['Background color', detail.backgroundColor],
    detail.foregroundColor && ['Foreground color', detail.foregroundColor],
  ].filter((row): row is string[] => Boolean(row));
  const referencedFieldIds = new Set(
    detail.layout?.items.flatMap(item => {
      const field = resolveLayoutField(item.target, detail.fields);
      return field ? [field.fieldId] : [];
    }) ?? [],
  );
  const unreferencedFields = detail.layout
    ? detail.fields.filter(field => !referencedFieldIds.has(field.fieldId))
    : detail.fields;
  const genericField = (field: EntryDetailResult['fields'][number]) => (
    <EntryFieldValue
      key={`${detail.id}:${field.fieldId}`}
      entryId={detail.id}
      field={field}
      label={getFieldName(field.kind, field.name)}
    />
  );
  return (
    <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
      <DetailSection title="Fields">
        <dl className="divide-y rounded-lg border">
          {detail.layout && <EntryLayoutView detail={detail} layout={detail.layout} />}
          {unreferencedFields.map(genericField)}
        </dl>
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
