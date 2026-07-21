import { Button } from '@/components/button';
import { Input } from '@/components/input';
import {
  Popover,
  PopoverContent,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from '@/components/popover';
import { Slider } from '@/components/slider';
import { ToggleGroup, ToggleGroupItem } from '@/components/toggle-group';
import { useShowToast } from '@/fragments/_providers/ToastProvider';
import { IconClipboardCheck, IconCopy, IconEye, IconEyeOff, IconRefreshCw, IconZap } from '@/icons';
import { useId, useState } from 'react';

const MIN_LENGTH = 8;
const MAX_LENGTH = 128;
const RANDOM_RANGE = 0x1_0000_0000;
const UPPERCASE = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
const LOWERCASE = 'abcdefghijklmnopqrstuvwxyz';
const NUMBERS = '0123456789';
const SHIFT_NUMBER_SYMBOLS = '!@#$%^&*()';
const OTHER_SYMBOLS = '`~-_=+[]{}\\|;:\'",.<>/?';
const AMBIGUOUS_CHARACTERS = new Set('oO0Il1');

type PasswordOptions = {
  length: number;
  uppercase: boolean;
  lowercase: boolean;
  numbers: boolean;
  shiftNumberSymbols: boolean;
  otherSymbols: boolean;
  includeAmbiguous: boolean;
};

type CharacterOption = Exclude<keyof PasswordOptions, 'length'>;

const DEFAULT_OPTIONS: PasswordOptions = {
  length: 24,
  uppercase: true,
  lowercase: true,
  numbers: true,
  shiftNumberSymbols: true,
  otherSymbols: false,
  includeAmbiguous: true,
};

const CHARACTER_OPTIONS: Array<{
  key: CharacterOption;
  label: string;
  description: string;
}> = [
  { key: 'uppercase', label: 'ABC', description: 'Include uppercase letters' },
  { key: 'lowercase', label: 'abc', description: 'Include lowercase letters' },
  { key: 'numbers', label: '123', description: 'Include numbers' },
  {
    key: 'shiftNumberSymbols',
    label: '!@#',
    description: 'Include Shift + number symbols',
  },
  {
    key: 'otherSymbols',
    label: '~;\\',
    description: 'Include other keyboard symbols',
  },
  {
    key: 'includeAmbiguous',
    label: 'oO0',
    description: 'Include ambiguous characters: o, O, 0, l, I, 1',
  },
];

const randomIndex = (length: number) => {
  const limit = RANDOM_RANGE - (RANDOM_RANGE % length);
  const value = new Uint32Array(1);
  do {
    crypto.getRandomValues(value);
  } while (value[0] >= limit);
  return value[0] % length;
};

const toSliderIndex = (value: number) => {
  if (value <= 16) {
    return value;
  }

  if (value <= 32) {
    return 16 + Math.round(value - 16) / 2;
  }

  return 24 + Math.round(value - 32) / 16;
};

const fromSliderIndex = (value: number) => {
  if (value <= 16) {
    return value;
  }

  if (value <= 24) {
    return 16 + (value - 16) * 2;
  }

  return 32 + (value - 24) * 16;
};

const generatePassword = (options: PasswordOptions) => {
  const characterGroups = [
    options.uppercase && UPPERCASE,
    options.lowercase && LOWERCASE,
    options.numbers && NUMBERS,
    options.shiftNumberSymbols && SHIFT_NUMBER_SYMBOLS,
    options.otherSymbols && OTHER_SYMBOLS,
  ]
    .filter((group): group is string => Boolean(group))
    .map(group =>
      !options.includeAmbiguous
        ? group
            .split('')
            .filter(character => !AMBIGUOUS_CHARACTERS.has(character))
            .join('')
        : group,
    );

  if (characterGroups.length === 0) {
    return '';
  }

  const allCharacters = characterGroups.join('');
  const password = characterGroups.map(group => group[randomIndex(group.length)]);
  while (password.length < options.length) {
    password.push(allCharacters[randomIndex(allCharacters.length)]);
  }
  for (let index = password.length - 1; index > 0; index -= 1) {
    const swapIndex = randomIndex(index + 1);
    [password[index], password[swapIndex]] = [password[swapIndex], password[index]];
  }
  return password.join('');
};

export const PasswordGenerator = ({
  name,
  disabled,
  onConfirm,
}: {
  name: string;
  disabled: boolean;
  onConfirm: (value: string) => void;
}) => {
  const lengthId = useId();
  const showToast = useShowToast();
  const [open, setOpen] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const [copied, setCopied] = useState(false);
  const [options, setOptions] = useState(DEFAULT_OPTIONS);
  const [password, setPassword] = useState(() => generatePassword(DEFAULT_OPTIONS));

  const refresh = (nextOptions = options) => {
    setPassword(generatePassword(nextOptions));
    setCopied(false);
  };

  const updateOptions = (nextOptions: PasswordOptions) => {
    setOptions(nextOptions);
    refresh(nextOptions);
  };

  const copyPassword = async () => {
    try {
      await navigator.clipboard.writeText(password);
      setCopied(true);
      showToast({ message: 'Generated password copied.', durationMs: 3000 });
    } catch {
      showToast({
        kind: 'destructive',
        message: 'Generated password could not be copied.',
      });
    }
  };

  return (
    <Popover
      open={open}
      onOpenChange={nextOpen => {
        setOpen(nextOpen);
        if (nextOpen) {
          refresh();
        }
      }}
    >
      <PopoverTrigger
        render={
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label={`Generate ${name}`}
            disabled={disabled}
          />
        }
      >
        <IconZap />
      </PopoverTrigger>
      <PopoverContent className="w-auto max-w-[calc(100vw-2rem)]" align="end">
        <PopoverHeader className="flex flex-row justify-between items-center">
          <PopoverTitle className="font-semibold">Password Generator</PopoverTitle>
        </PopoverHeader>
        <div className="flex items-center gap-1 rounded-lg border border-input p-1">
          <Input
            type={revealed ? 'text' : 'password'}
            value={password}
            className="border-0 bg-transparent font-mono shadow-none focus-visible:ring-0 dark:bg-transparent"
            aria-label="Generated password preview"
            readOnly
          />
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label={revealed ? 'Hide generated password' : 'Reveal generated password'}
            aria-pressed={revealed}
            disabled={!password}
            onClick={() => setRevealed(current => !current)}
          >
            {revealed ? <IconEyeOff /> : <IconEye />}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Copy generated password"
            disabled={!password}
            onClick={copyPassword}
          >
            {copied ? <IconClipboardCheck /> : <IconCopy />}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            aria-label="Generate another password"
            disabled={!password}
            onClick={() => refresh()}
          >
            <IconRefreshCw />
          </Button>
        </div>

        <span className="font-semibold">Options</span>

        <ToggleGroup
          multiple
          variant="outline"
          value={CHARACTER_OPTIONS.map(({ key }) => key).filter(key => options[key])}
          onValueChange={values =>
            updateOptions({
              ...options,
              ...Object.fromEntries(
                CHARACTER_OPTIONS.map(({ key }) => [key, values.includes(key)]),
              ),
            })
          }
        >
          {CHARACTER_OPTIONS.map(option => (
            <ToggleGroupItem
              key={option.key}
              className="w-10 h-10"
              value={option.key}
              onClick={() => updateOptions({ ...options, [option.key]: !options[option.key] })}
            >
              {option.label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>

        <div className="flex flex-col">
          <label htmlFor={lengthId} className="sr-only">
            Length
          </label>
          <div className="flex items-center gap-2">
            <Slider
              id={lengthId}
              min={toSliderIndex(MIN_LENGTH)}
              max={toSliderIndex(MAX_LENGTH)}
              value={toSliderIndex(options.length)}
              variant="contrast"
              className="flex-1 disabled:cursor-not-allowed disabled:opacity-50"
              disabled={disabled}
              onValueChange={value =>
                updateOptions({ ...options, length: fromSliderIndex(value as number) })
              }
            />
            <Input
              id={lengthId}
              type="number"
              min={MIN_LENGTH}
              max={MAX_LENGTH}
              value={options.length}
              className="min-w-0 flex-[0_0_4.5rem] accent-primary disabled:cursor-not-allowed disabled:opacity-50"
              disabled={disabled}
              onChange={event => updateOptions({ ...options, length: event.target.valueAsNumber })}
            />
          </div>
        </div>

        <Button
          type="button"
          variant="contrast"
          className="w-full"
          disabled={disabled || !password}
          onClick={() => {
            onConfirm(password);
            setOpen(false);
          }}
        >
          Done
        </Button>

        {!password && (
          <p className="text-xs text-destructive" role="alert">
            Select at least one character group.
          </p>
        )}
      </PopoverContent>
    </Popover>
  );
};
