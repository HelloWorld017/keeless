import { buildRoute } from '@/utils/route';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { useHost } from '../_providers/HostProvider';
import { useNavigate } from '../_providers/RouterProvider';
import {invalidateByResource} from '@/utils/request';

export const EntryFocusHandler = () => {
  const host = useHost();
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!host?.onEntryFocus) {
      return undefined;
    }

    return host.onEntryFocus(entryId => {
      void invalidateByResource(queryClient, ['databaseStatus', 'group', 'entry', 'tag'])
        .then(() => {
          const entryRoute = `${buildRoute('database')}?entry=${encodeURIComponent(entryId)}`;
          navigate(entryRoute, { replace: true });
        });
    });
  }, [host, navigate, queryClient]);

  return null;
};
