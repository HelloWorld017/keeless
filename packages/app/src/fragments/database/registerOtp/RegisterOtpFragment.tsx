import { Button } from '@/components/button';
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/collapsible';
import { Field, FieldDescription, FieldLabel } from '@/components/field';
import { Input } from '@/components/input';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/select';
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/tabs';
import {
  AdaptiveSheet,
  AdaptiveSheetContent,
  AdaptiveSheetDescription,
  AdaptiveSheetHeader,
  AdaptiveSheetOverlay,
  AdaptiveSheetTitle,
} from '@/fragments/_components/AdaptiveSheet';
import { QRScanner } from '@/fragments/_components/QRScanner';
import { IconChevronRight } from '@/icons';
import { cx } from '@/utils/css';
import { useState } from 'react';

type Tab = 'scan' | 'upload' | 'paste';

const base32 = (value: string) => value.replace(/[\s-]/g, '').toUpperCase().replace(/=+$/, '');

const normalizeUri = (value: string) => {
  try {
    const uri = new URL(value.trim());
    if (uri.protocol !== 'otpauth:' || uri.hostname.toLowerCase() !== 'totp') {
      return undefined;
    }
    const secret = base32(uri.searchParams.get('secret') ?? '');
    if (!/^[A-Z2-7]+$/.test(secret)) {
      return undefined;
    }
    const algorithm = (uri.searchParams.get('algorithm') ?? 'SHA1').toUpperCase();
    const period = Number(uri.searchParams.get('period') ?? '30');
    const digits = Number(uri.searchParams.get('digits') ?? '6');
    if (
      !['SHA1', 'SHA256', 'SHA512'].includes(algorithm) ||
      !Number.isInteger(period) ||
      period <= 0 ||
      ![6, 7, 8].includes(digits)
    ) {
      return undefined;
    }
    uri.searchParams.set('secret', secret);
    uri.searchParams.set('algorithm', algorithm);
    uri.searchParams.set('period', String(period));
    uri.searchParams.set('digits', String(digits));
    return uri.toString();
  } catch {
    return undefined;
  }
};

const fromSecret = ({
  secret,
  algorithm,
  period,
  digits,
}: {
  secret: string;
  algorithm: string;
  period: string;
  digits: string;
}) => {
  const normalized = base32(secret);
  if (!/^[A-Z2-7]+$/.test(normalized)) {
    return undefined;
  }
  const uri = new URL('otpauth://totp/Keeless');
  uri.searchParams.set('secret', normalized);
  uri.searchParams.set('algorithm', algorithm);
  uri.searchParams.set('period', period);
  uri.searchParams.set('digits', digits);
  return normalizeUri(uri.toString());
};

export const RegisterOtpFragment = ({
  open,
  onOpenChange,
  onConfirm,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (uri: string) => void;
}) => {
  const [tab, setTab] = useState<Tab>('scan');
  const [paste, setPaste] = useState('');
  const [secret, setSecret] = useState('');
  const [advanced, setAdvanced] = useState(false);
  const [algorithm, setAlgorithm] = useState('SHA1');
  const [period, setPeriod] = useState('30');
  const [digits, setDigits] = useState('6');
  const [pasteError, setPasteError] = useState<string>();

  const finish = (value: string) => {
    const uri = normalizeUri(value);
    if (!uri) {
      return 'The QR code does not contain a valid TOTP setup.';
    }
    onConfirm(uri);
    onOpenChange(false);
    return undefined;
  };

  const submitPaste = () => {
    const uri = paste.trim()
      ? normalizeUri(paste)
      : fromSecret({ secret, algorithm, period, digits });
    if (!uri) {
      setPasteError('Enter a valid TOTP URI or Base32 secret.');
      return;
    }
    onConfirm(uri);
    onOpenChange(false);
  };

  return (
    <AdaptiveSheet open={open} onOpenChange={onOpenChange}>
      <AdaptiveSheetOverlay forceRender />
      <AdaptiveSheetContent className="max-h-[min(44rem,calc(100dvh-1rem))] overflow-y-auto p-4 sm:p-6">
        <AdaptiveSheetHeader className="p-0">
          <AdaptiveSheetTitle>Register OTP</AdaptiveSheetTitle>
          <AdaptiveSheetDescription>
            Add a TOTP setup from a QR code or secret key.
          </AdaptiveSheetDescription>
        </AdaptiveSheetHeader>
        <Tabs>
          <TabsList>
            {(['scan', 'upload', 'paste'] as const).map(value => (
              <TabsTrigger key={value} active={tab === value} onClick={() => setTab(value)}>
                {value[0].toUpperCase() + value.slice(1)}
              </TabsTrigger>
            ))}
          </TabsList>
          {tab !== 'paste' && (
            <TabsContent>
              <QRScanner active={open} mode={tab} onScan={finish} />
            </TabsContent>
          )}
          {tab === 'paste' && (
            <TabsContent className="space-y-4">
              <Field>
                <FieldLabel htmlFor="otp-uri">TOTP URI</FieldLabel>
                <Input
                  id="otp-uri"
                  value={paste}
                  placeholder="otpauth://totp/..."
                  autoComplete="off"
                  spellCheck={false}
                  onChange={event => {
                    setPaste(event.target.value);
                    setPasteError(undefined);
                  }}
                />
                <FieldDescription>
                  Paste the full provisioning URI, or enter a Base32 secret below.
                </FieldDescription>
              </Field>
              <Field>
                <FieldLabel htmlFor="otp-secret">Base32 secret</FieldLabel>
                <Input
                  id="otp-secret"
                  value={secret}
                  placeholder="JBSWY3DPEHPK3PXP"
                  autoComplete="off"
                  spellCheck={false}
                  onChange={event => {
                    setSecret(event.target.value);
                    setPasteError(undefined);
                  }}
                />
              </Field>
              {!paste.trim() && (
                <Collapsible open={advanced} onOpenChange={setAdvanced}>
                  <CollapsibleTrigger render={<Button type="button" variant="ghost" size="sm" />}>
                    <IconChevronRight
                      className={cx('transition-transform', advanced && 'rotate-90')}
                    />
                    Advanced options
                  </CollapsibleTrigger>
                  <CollapsibleContent className="grid gap-3 pt-3 sm:grid-cols-3">
                    <Field>
                      <FieldLabel>Algorithm</FieldLabel>
                      <Select
                        value={algorithm}
                        onValueChange={value => value && setAlgorithm(value)}
                      >
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="SHA1">SHA-1</SelectItem>
                          <SelectItem value="SHA256">SHA-256</SelectItem>
                          <SelectItem value="SHA512">SHA-512</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                    <Field>
                      <FieldLabel htmlFor="otp-period">Time step</FieldLabel>
                      <Input
                        id="otp-period"
                        type="number"
                        min="1"
                        value={period}
                        onChange={event => setPeriod(event.target.value)}
                      />
                    </Field>
                    <Field>
                      <FieldLabel>Code size</FieldLabel>
                      <Select value={digits} onValueChange={value => value && setDigits(value)}>
                        <SelectTrigger>
                          <SelectValue />
                        </SelectTrigger>
                        <SelectContent>
                          <SelectItem value="6">6 digits</SelectItem>
                          <SelectItem value="7">7 digits</SelectItem>
                          <SelectItem value="8">8 digits</SelectItem>
                        </SelectContent>
                      </Select>
                    </Field>
                  </CollapsibleContent>
                </Collapsible>
              )}
              {pasteError && (
                <p className="text-sm text-destructive" role="alert">
                  {pasteError}
                </p>
              )}
              <Button type="button" className="w-full" onClick={submitPaste}>
                Use OTP setup
              </Button>
            </TabsContent>
          )}
        </Tabs>
      </AdaptiveSheetContent>
    </AdaptiveSheet>
  );
};
