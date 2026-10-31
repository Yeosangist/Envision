
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, useState } from 'react';
import type { ScanProgress } from '../types';

export const SCAN_EVENT = 'scan:progress';

export const CHANGED_EVENT = 'library:changed';

export interface LibraryChangedEvent {
  library_id: string;
  paths: string[];
}

export function useScanProgress(): ScanProgress | null {
  const [progress, setProgress] = useState<ScanProgress | null>(null);

  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    listen<ScanProgress>(SCAN_EVENT, (event) => {
      if (!cancelled) setProgress(event.payload);
    })
      .then((dispose) => {
        if (cancelled) dispose();
        else unlisten = dispose;
      })
      .catch(() => {
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return progress;
}

export function useLibraryChanged(onChanged: (event: LibraryChangedEvent) => void): void {
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;

    listen<LibraryChangedEvent>(CHANGED_EVENT, (event) => {
      if (!cancelled) onChanged(event.payload);
    })
      .then((dispose) => {
        if (cancelled) dispose();
        else unlisten = dispose;
      })
      .catch(() => {});

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onChanged]);
}