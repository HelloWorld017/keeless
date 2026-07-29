import { FieldPlain } from '../_components/FieldPlain';
import { formatDate } from '@/utils/format';

export const ExpiryValue = ({
  label,
  expires,
  expiryTimeMs,
}: {
  label: string;
  expires: boolean;
  expiryTimeMs: number | null;
}) => <FieldPlain name={label} value={expires ? formatDate(expiryTimeMs) : 'Never'} />;
