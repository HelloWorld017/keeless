import { buildRoute } from '@/utils/route';
import { useQueryClient } from '@tanstack/react-query';
import { useEffect } from 'react';
import { useHost } from '../_providers/HostProvider';
import { useNavigate } from '../_providers/RouterProvider';

export const EntryFocusHandler = () => {
  const host = useHost();
  const navigate = useNavigate();
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!host?.onEntryFocus) {
      return undefined;
    }

    return host.onEntryFocus(entryId => {
      void Promise.all([
        queryClient.invalidateQueries({ queryKey: ['request', 'getEntries'] }),
        queryClient.invalidateQueries({ queryKey: ['request', 'searchEntries'] }),
        queryClient.invalidateQueries({ queryKey: ['request', 'getGroupEntries'] }),
        queryClient.invalidateQueries({ queryKey: ['request', 'getEntryTemplates'] }),
        queryClient.invalidateQueries({ queryKey: ['request', 'getTagEntries'] }),
        queryClient.invalidateQueries({ queryKey: ['request', 'getTags'] }),
      ]).then(() => {
        const entryRoute = `${buildRoute('database')}?entry=${encodeURIComponent(entryId)}`;
        navigate(entryRoute, { replace: true });
      });
    });
  }, [host, navigate, queryClient]);

  return null;
};
