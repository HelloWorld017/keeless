import { CoreRequestError } from '@/utils/request';

export const errorMessage = (error: unknown) => {
  if (error instanceof CoreRequestError) {
    switch (error.code) {
      case 'invalid_credentials':
        return 'That password could not unlock this database.';
      case 'database_not_found':
        return 'The database no longer exists.';
      case 'storage_error':
        return 'The storage could not be accessed.';
      default:
        return error.message;
    }
  }
  return error instanceof Error ? error.message : 'An unexpected error occurred.';
};
