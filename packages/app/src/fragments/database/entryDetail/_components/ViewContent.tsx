import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import {
  Attachment,
  AttachmentContent,
  AttachmentDescription,
  AttachmentMedia,
  AttachmentTitle,
  AttachmentTrigger,
} from '@/components/attachment';
import { Button } from '@/components/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/collapsible';
import { useRequest, useRequestClient } from '@/fragments/_providers/QueryProvider';
import { useShowToast } from '@/fragments/_providers/ToastProvider';
import { IconChevronRight, IconFile, IconInfo, IconLoaderCircle } from '@/icons';
import { cn } from '@/utils/css';
import { formatBytes, formatDate } from '@/utils/format';
import { useState, type ReactNode } from 'react';
import { Tag } from '../../_components/Tag';
import { ExpiryValue } from '../_layout/ExpiryValue';
import { FieldDivider } from '../_layout/FieldDivider';
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
  const [downloadingAttachment, setDownloadingAttachment] = useState<number>();
  const tags = useRequest('getTags', {});
  const requestClient = useRequestClient();
  const showToast = useShowToast();
  const tagStyle = (name: string) => tags.data?.tags.find(tag => tag.name === name)?.style;
  const colorRows = [
    detail.backgroundColor && ['Background color', detail.backgroundColor],
    detail.foregroundColor && ['Foreground color', detail.foregroundColor],
  ].filter((row): row is string[] => Boolean(row));
  const fields = detail.fields.toSorted((left, right) => left.order - right.order);
  const internalFields = fields.filter(field => field.type === 'field' && field.isInternal);
  const visibleFields = fields.filter(field => field.type !== 'field' || !field.isInternal);
  const hasTagField = fields.some(field => field.type === 'tags');
  const downloadAttachment = async (attachment: EntryAttachmentInformation) => {
    setDownloadingAttachment(attachment.index);
    try {
      const transfer = await requestClient.data!.request('prepareEntryAttachmentDownload', {
        entryId: detail.id,
        attachmentIndex: attachment.index,
        name: attachment.name,
      });
      const bytes = await requestClient.data!.download(transfer.transferId);
      try {
        const url = URL.createObjectURL(new Blob([bytes], { type: 'application/octet-stream' }));
        const anchor = document.createElement('a');
        anchor.href = url;
        anchor.download = attachment.name || 'attachment';
        anchor.click();
        setTimeout(() => URL.revokeObjectURL(url), 0);
      } finally {
        bytes.fill(0);
      }
    } catch {
      showToast({ kind: 'destructive', message: 'The attachment could not be downloaded.' });
    } finally {
      setDownloadingAttachment(undefined);
    }
  };
  const tagList = (key: string, label?: string) => (
    <div key={key} className="space-y-2 px-4 py-3">
      {label && <dt className="text-xs text-muted-foreground">{label}</dt>}
      <dd className="flex flex-wrap gap-1.5">
        {detail.tags.map(name => (
          <Tag key={name} name={name} tagStyle={tagStyle(name)} />
        ))}
      </dd>
    </div>
  );

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
        return tagList(key, field.label);
      case 'field':
        return (
          <EntryFieldValue
            key={key}
            field={field}
            label={field.control ? field.label : getFieldName(field.kind, field.name)}
            control={field.control}
          />
        );
      default:
        return null;
    }
  };

  return (
    <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
      {detail.isTemplate && (
        <Alert>
          <IconInfo />
          <AlertTitle>This is a template</AlertTitle>
          <AlertDescription>
            Changes can affect the layout and behavior of entries that use this template.
          </AlertDescription>
        </Alert>
      )}
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
      {detail.tags.length > 0 && !hasTagField && (
        <DetailSection title="Tags">
          <div className="flex flex-wrap gap-1.5">
            {detail.tags.map(name => (
              <Tag key={name} name={name} tagStyle={tagStyle(name)} />
            ))}
          </div>
        </DetailSection>
      )}
      {detail.attachments.length > 0 && (
        <DetailSection title="Attachments">
          <div className="divide-y rounded-lg border">
            {detail.attachments.map(attachment => (
              <Attachment
                key={`${attachment.index}:${attachment.name}`}
                state={downloadingAttachment === attachment.index ? 'uploading' : 'done'}
                className="w-full rounded-none border-0"
              >
                <AttachmentMedia>
                  {downloadingAttachment === attachment.index ? (
                    <IconLoaderCircle className="animate-spin" />
                  ) : (
                    <IconFile />
                  )}
                </AttachmentMedia>
                <AttachmentContent>
                  <AttachmentTitle>{attachment.name || 'Untitled attachment'}</AttachmentTitle>
                  <AttachmentDescription>
                    {attachment.isProtected && 'Protected · '}
                    {formatBytes(attachment.size)}
                  </AttachmentDescription>
                </AttachmentContent>
                <AttachmentTrigger
                  aria-label={`Download ${attachment.name || 'attachment'}`}
                  disabled={downloadingAttachment !== undefined}
                  onClick={() => void downloadAttachment(attachment)}
                />
              </Attachment>
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
