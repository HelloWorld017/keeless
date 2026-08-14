import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export const cx = (...inputs: ClassValue[]) => twMerge(clsx(inputs));
export { cx as cn };
