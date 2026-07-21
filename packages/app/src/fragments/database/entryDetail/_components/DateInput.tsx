import { Button } from '@/components/button';
import { Calendar } from '@/components/calendar';
import { Input } from '@/components/input';
import { Popover, PopoverContent, PopoverTrigger } from '@/components/popover';
import { IconCalendar, IconX } from '@/icons';
import { cn } from '@/utils/css';
import { format } from 'date-fns';
import { useState } from 'react';

const parseDate = (value: string) => {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(value);
  if (!match) {
    return undefined;
  }
  const [year, month, day] = match.slice(1).map(Number);
  const date = new Date(0);
  date.setFullYear(year, month - 1, day);
  date.setHours(0, 0, 0, 0);
  return Number.isNaN(date.getTime()) ||
    date.getFullYear() !== year ||
    date.getMonth() !== month - 1 ||
    date.getDate() !== day
    ? undefined
    : date;
};

const dateValue = (date: Date) =>
  [
    date.getFullYear().toString().padStart(4, '0'),
    (date.getMonth() + 1).toString().padStart(2, '0'),
    date.getDate().toString().padStart(2, '0'),
  ].join('-');

export const DateInput = ({
  id,
  value,
  dateTime = false,
  disabled = false,
  onChange,
}: {
  id: string;
  value: string;
  dateTime?: boolean;
  disabled?: boolean;
  onChange: (value: string) => void;
}) => {
  const [open, setOpen] = useState(false);
  const selected = parseDate(value);
  const time = /^\d{4}-\d{2}-\d{2}T(\d{2}:\d{2})/.exec(value)?.[1] ?? '00:00';
  const displayValue = selected
    ? `${format(selected, 'PPP')}${dateTime ? `, ${time}` : ''}`
    : dateTime
      ? 'Pick a date and time'
      : 'Pick a date';

  return (
    <div className="flex gap-2">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <Button
              id={id}
              type="button"
              variant="outline"
              className={cn(
                'min-w-0 flex-1 justify-start font-normal',
                !selected && 'text-muted-foreground',
              )}
              disabled={disabled}
            />
          }
        >
          <IconCalendar />
          <span className="truncate">{displayValue}</span>
        </PopoverTrigger>
        <PopoverContent className="w-auto p-0" align="start">
          <Calendar
            mode="single"
            selected={selected}
            defaultMonth={selected}
            onSelect={date => {
              if (!date) {
                return;
              }
              onChange(`${dateValue(date)}${dateTime ? `T${time}` : ''}`);
              if (!dateTime) {
                setOpen(false);
              }
            }}
          />
          {dateTime && (
            <div className="border-t p-3">
              <Input
                type="time"
                aria-label="Time"
                value={time}
                disabled={!selected}
                onChange={event => {
                  if (selected) {
                    onChange(`${dateValue(selected)}T${event.target.value || '00:00'}`);
                  }
                }}
              />
            </div>
          )}
        </PopoverContent>
      </Popover>
      {value && (
        <Button
          type="button"
          variant="ghost"
          size="icon"
          aria-label="Clear date"
          disabled={disabled}
          onClick={() => onChange('')}
        >
          <IconX />
        </Button>
      )}
    </div>
  );
};
