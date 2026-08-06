import { Alert, AlertDescription, AlertTitle } from '@/components/alert';
import {
  Attachment,
  AttachmentAction,
  AttachmentActions,
  AttachmentContent,
  AttachmentDescription,
  AttachmentMedia,
  AttachmentTitle,
} from '@/components/attachment';
import { Button } from '@/components/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/collapsible';
import { Field, FieldError, FieldGroup, FieldLabel } from '@/components/field';
import {
  FileUpload,
  FileUploadClear,
  FileUploadDropzone,
  FileUploadItem,
  FileUploadItemDelete,
  FileUploadItemMetadata,
  FileUploadItemPreview,
  FileUploadList,
  FileUploadTrigger,
} from '@/components/file-upload';
import { Input } from '@/components/input';
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/popover';
import { Toggle } from '@/components/toggle';
import {
  IconChevronRight,
  IconFile,
  IconLockKeyhole,
  IconLockKeyholeOpen,
  IconTrash,
  IconTriangleAlert,
} from '@/icons';
import { cx } from '@/utils/css';
import { formatBytes } from '@/utils/format';
import { useState } from 'react';
import { TagPicker } from '../../_components/TagPicker';
import { ExpiryEditor } from '../_layout/ExpiryEditor';
import { FieldDivider } from '../_layout/FieldDivider';
import { PasswordConfirmationEditor } from '../_layout/PasswordConfirmationEditor';
import { getFieldName } from '../_utils/getFieldName';
import { AddField } from './AddField';
import { EntryFieldEditor } from './EntryFieldEditor';
import type { EntryPropertiesDraft } from '../_types/EntryPropertiesDraft';
import type { FieldDraft } from '../_types/FieldDraft';
import type {
  DatabaseNodeId,
  EntryAttachmentInformation,
  EntryFieldInformation,
} from '@keeless/schema';

const LARGE_ATTACHMENT_SIZE = 512 * 1024;

const AttachmentSizeWarning = () => (
  <Popover>
    <PopoverTrigger
      render={
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          className="ml-1 inline-flex shrink-0 align-middle text-amber-600 hover:text-amber-600 dark:text-amber-400 dark:hover:text-amber-400"
        />
      }
      aria-label="Large attachment performance warning"
    >
      <IconTriangleAlert />
    </PopoverTrigger>
    <PopoverContent className="w-64">
      <PopoverHeader>
        <PopoverTitle>Large attachment</PopoverTitle>
        <PopoverDescription>
          Attachments larger than 512KiB may cause performance issues.
        </PopoverDescription>
      </PopoverHeader>
    </PopoverContent>
  </Popover>
);

type OrderedItem =
  | { type: 'draft'; order: number; draft: FieldDraft }
  | { type: 'template'; order: number; field: Exclude<EntryFieldInformation, { type: 'field' }> };

export const EditContent = ({
  entryId,
  isTemplate,
  drafts,
  fields,
  properties,
  confirmations,
  confirmationErrors,
  attachments,
  existingAttachments,
  errors,
  pending,
  onAdd,
  onAddOtp,
  onAddUrl,
  onLoad,
  onChange,
  onDelete,
  onPropertiesChange,
  onConfirmationChange,
  onAttachmentsChange,
  onExistingAttachmentDelete,
}: {
  entryId: DatabaseNodeId;
  isTemplate: boolean;
  drafts: FieldDraft[];
  fields: EntryFieldInformation[];
  properties: EntryPropertiesDraft;
  confirmations: Record<string, string>;
  confirmationErrors: Set<string>;
  attachments: File[];
  existingAttachments: EntryAttachmentInformation[];
  errors: Set<string>;
  pending: boolean;
  onAdd: () => void;
  onAddOtp: () => void;
  onAddUrl: () => void;
  onLoad: (key: string, value: string) => void;
  onChange: (key: string, patch: Partial<FieldDraft>) => void;
  onDelete: (key: string) => void;
  onPropertiesChange: (patch: Partial<EntryPropertiesDraft>) => void;
  onConfirmationChange: (fieldId: string, value: string) => void;
  onAttachmentsChange: (attachments: File[]) => void;
  onExistingAttachmentDelete: (attachmentIndex: number) => void;
}) => {
  const [internalOpen, setInternalOpen] = useState(false);
  const ordered: OrderedItem[] = [
    ...drafts.map(draft => ({ type: 'draft' as const, order: draft.order, draft })),
    ...fields
      .filter(field => field.type !== 'field')
      .map(field => ({ type: 'template' as const, order: field.order, field })),
  ].toSorted((left, right) => left.order - right.order);
  const internal = ordered.filter(item => item.type === 'draft' && item.draft.isInternal);
  const visible = ordered.filter(item => item.type !== 'draft' || !item.draft.isInternal);
  const hasExpiry = fields.some(field => field.type === 'expiry');
  const hasTags = fields.some(field => field.type === 'tags');

  const renderItem = (item: OrderedItem) => {
    if (item.type === 'template') {
      const { field } = item;
      const id = `template-${field.order}`;
      if (field.type === 'timeOtp') {
        return null;
      }
      if (field.type === 'divider') {
        return <FieldDivider key={id} label={field.label} editing />;
      }
      if (field.type === 'passwordConfirmation') {
        return (
          <PasswordConfirmationEditor
            key={id}
            id={`${id}-confirmation`}
            label={field.label}
            value={confirmations[field.passwordFieldId] ?? ''}
            invalid={confirmationErrors.has(field.passwordFieldId)}
            pending={pending}
            onChange={value => onConfirmationChange(field.passwordFieldId, value)}
          />
        );
      }
      if (field.type === 'tags') {
        return (
          <Field key={id}>
            <FieldLabel htmlFor={id}>{field.label}</FieldLabel>
            <TagPicker
              id={id}
              value={properties.tags}
              disabled={pending}
              onChange={tags => onPropertiesChange({ tags, tagsChanged: true })}
            />
          </Field>
        );
      }
      if (field.type === 'overrideUrl') {
        return (
          <Field key={id}>
            <FieldLabel htmlFor={id}>{field.label}</FieldLabel>
            <Input
              id={id}
              type="url"
              value={properties.overrideUrl}
              disabled={pending}
              onChange={event => onPropertiesChange({ overrideUrl: event.target.value })}
            />
          </Field>
        );
      }
      return (
        <ExpiryEditor
          key={id}
          id={`${id}-expiry`}
          label={field.label}
          control={field.control}
          properties={properties}
          pending={pending}
          onChange={onPropertiesChange}
        />
      );
    }

    const { draft } = item;
    const standard = draft.kind !== 'custom';
    const configured = draft.control !== null;
    const canBeProtected = !configured && (draft.kind === 'custom' || draft.kind === 'notes');
    const invalid = errors.has(draft.key);
    const nameId = `${draft.key}-name`;
    const valueId = `${draft.key}-value`;
    const errorId = `${draft.key}-error`;
    const editorName = configured
      ? draft.label
      : getFieldName(draft.kind, draft.name) || 'Custom field';
    const editor = (
      <EntryFieldEditor
        entryId={entryId}
        draft={draft}
        label={editorName}
        control={draft.control ?? undefined}
        placeholder={standard || configured ? undefined : 'Enter a value'}
        pending={pending}
        onLoad={value => onLoad(draft.key, value)}
        onChange={value => onChange(draft.key, { value, valueChanged: true })}
      />
    );
    const fieldLabel = (
      <div className="flex items-center gap-1">
        <FieldLabel htmlFor={valueId}>{editorName}</FieldLabel>
        {canBeProtected && (
          <Toggle
            type="button"
            size="sm"
            pressed={draft.isProtected}
            className="min-w-0 size-7 p-0 -my-3"
            aria-label={`Protect ${editorName}`}
            disabled={pending}
            onPressedChange={isProtected => onChange(draft.key, { isProtected })}
          >
            {draft.isProtected ? <IconLockKeyhole /> : <IconLockKeyholeOpen />}
          </Toggle>
        )}
      </div>
    );

    return (
      <div key={draft.key}>
        {standard || configured ? (
          <Field>
            {fieldLabel}
            {editor}
          </Field>
        ) : (
          <FieldGroup className="gap-3">
            {fieldLabel}
            <div className="flex items-center gap-2">
              <Field data-invalid={invalid} className="min-w-0 flex-1 gap-1.5">
                <Input
                  id={nameId}
                  value={draft.name}
                  placeholder="e.g. Recovery email"
                  aria-invalid={invalid}
                  aria-describedby={invalid ? errorId : undefined}
                  disabled={pending}
                  onChange={event => onChange(draft.key, { name: event.target.value })}
                />
                {invalid && (
                  <FieldError id={errorId} className="text-xs">
                    Enter a field name.
                  </FieldError>
                )}
              </Field>
              <Field className="flex-2">{editor}</Field>
              <Button
                type="button"
                variant="ghost"
                size="icon-sm"
                className="text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                aria-label={`Delete ${draft.name || 'custom field'}`}
                disabled={pending}
                onClick={() => onDelete(draft.key)}
              >
                <IconTrash />
              </Button>
            </div>
          </FieldGroup>
        )}
      </div>
    );
  };

  return (
    <div className="w-full max-w-3xl space-y-8 p-4 sm:p-6">
      {isTemplate && (
        <Alert>
          <IconTriangleAlert />
          <AlertTitle>You are editing a template</AlertTitle>
          <AlertDescription>
            Changes can affect the layout and behavior of entries that use this template.
          </AlertDescription>
        </Alert>
      )}
      <FieldGroup className="gap-5">{visible.map(renderItem)}</FieldGroup>
      {internal.length > 0 && (
        <Collapsible open={internalOpen} onOpenChange={setInternalOpen}>
          <CollapsibleTrigger
            render={<Button type="button" variant="ghost" className="w-full justify-start" />}
          >
            <IconChevronRight className={cx('transition-transform', internalOpen && 'rotate-90')} />
            Internal fields
            <span className="text-muted-foreground">({internal.length})</span>
          </CollapsibleTrigger>
          <CollapsibleContent className="pt-4">
            <FieldGroup className="gap-5">{internal.map(renderItem)}</FieldGroup>
          </CollapsibleContent>
        </Collapsible>
      )}
      <AddField pending={pending} onAddGeneric={onAdd} onAddOtp={onAddOtp} onAddUrl={onAddUrl} />
      <section className="space-y-4">
        <div>
          <h2 className="text-sm font-semibold">Attachments</h2>
        </div>
        {existingAttachments.length > 0 && (
          <div className="divide-y rounded-lg border">
            {existingAttachments.map(attachment => (
              <Attachment
                key={`${attachment.index}:${attachment.name}`}
                className="w-full rounded-lg border-0 bg-transparent"
              >
                <AttachmentMedia>
                  <IconFile />
                </AttachmentMedia>
                <AttachmentContent>
                  <AttachmentTitle>{attachment.name || 'Untitled attachment'}</AttachmentTitle>
                  <AttachmentDescription className="flex items-center">
                    <span className="min-w-0 truncate">
                      {attachment.isProtected && 'Protected · '}
                      {formatBytes(attachment.size)}
                    </span>
                    {attachment.size > LARGE_ATTACHMENT_SIZE && <AttachmentSizeWarning />}
                  </AttachmentDescription>
                </AttachmentContent>
                <AttachmentActions>
                  <AttachmentAction
                    type="button"
                    size="icon-lg"
                    className="hover:bg-destructive/10 hover:text-destructive"
                    aria-label={`Remove ${attachment.name || 'attachment'}`}
                    disabled={pending}
                    onClick={() => onExistingAttachmentDelete(attachment.index)}
                  >
                    <IconTrash />
                  </AttachmentAction>
                </AttachmentActions>
              </Attachment>
            ))}
          </div>
        )}
        <FileUpload
          value={attachments}
          multiple
          disabled={pending}
          onValueChange={onAttachmentsChange}
        >
          <FileUploadDropzone>
            <IconFile className="text-2xl text-muted-foreground" />
            <div className="text-muted-foreground text-center">
              Drop attachments here
              <br />
              or <FileUploadTrigger className="font-semibold">choose</FileUploadTrigger> a file.
            </div>
          </FileUploadDropzone>
          <FileUploadList>
            {attachments.map((attachment, index) => (
              <FileUploadItem key={`${attachment.name}:${index}`} value={attachment}>
                <FileUploadItemPreview className="[:has(svg)]:border-none" />
                <FileUploadItemMetadata
                  sizeEndAdornment={
                    attachment.size > LARGE_ATTACHMENT_SIZE ? <AttachmentSizeWarning /> : undefined
                  }
                />
                <FileUploadItemDelete
                  aria-label={`Remove ${attachment.name}`}
                  disabled={pending}
                  render={<Button type="button" variant="ghost" size="icon-sm" />}
                >
                  <IconTrash />
                </FileUploadItemDelete>
              </FileUploadItem>
            ))}
          </FileUploadList>
          <FileUploadClear render={<Button type="button" variant="outline" size="sm" />}>
            Clear uploads
          </FileUploadClear>
        </FileUpload>
      </section>
      {(!hasExpiry || !hasTags) && (
        <section className="space-y-4">
          <h2 className="text-sm font-semibold">Additional properties</h2>
          <FieldGroup className="gap-5">
            {!hasExpiry && (
              <ExpiryEditor
                id="additional-expiry"
                label="Expires"
                control={{ type: 'date' }}
                properties={properties}
                pending={pending}
                onChange={onPropertiesChange}
              />
            )}
            {!hasTags && (
              <Field>
                <FieldLabel htmlFor="additional-tags">Tags</FieldLabel>
                <TagPicker
                  id="additional-tags"
                  value={properties.tags}
                  disabled={pending}
                  onChange={tags => onPropertiesChange({ tags, tagsChanged: true })}
                />
              </Field>
            )}
          </FieldGroup>
        </section>
      )}
    </div>
  );
};
