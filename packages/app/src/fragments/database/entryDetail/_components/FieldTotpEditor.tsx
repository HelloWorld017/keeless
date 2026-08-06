import { Button } from '@/components/button';
import { RegisterOtpFragment } from '@/fragments/database/registerOtp/RegisterOtpFragment';
import { IconPencil } from '@/icons';
import { useState } from 'react';

export const FieldTotpEditor = ({
  disabled,
  onChange,
}: {
  disabled: boolean;
  onChange: (value: string) => void;
}) => {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button type="button" variant="outline" disabled={disabled} onClick={() => setOpen(true)}>
        <IconPencil />
        Edit OTP
      </Button>
      <RegisterOtpFragment
        open={open}
        onOpenChange={setOpen}
        onConfirm={value => onChange(value)}
      />
    </>
  );
};
