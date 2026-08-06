import { Button } from '@/components/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/dropdown-menu';
import { IconPlus } from '@/icons';

export const AddField = ({
  pending,
  onAddGeneric,
  onAddOtp,
  onAddUrl,
}: {
  pending: boolean;
  onAddGeneric: () => void;
  onAddOtp: () => void;
  onAddUrl: () => void;
}) => (
  <DropdownMenu>
    <DropdownMenuTrigger
      render={
        <Button
          type="button"
          variant="outline"
          className="w-full border-dashed text-muted-foreground hover:text-foreground"
          disabled={pending}
        />
      }
    >
      <IconPlus />
      Add field
    </DropdownMenuTrigger>
    <DropdownMenuContent className="min-w-44">
      <DropdownMenuItem onClick={onAddGeneric}>Generic</DropdownMenuItem>
      <DropdownMenuItem onClick={onAddUrl}>URL</DropdownMenuItem>
      <DropdownMenuItem onClick={onAddOtp}>One-time Password</DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
);
