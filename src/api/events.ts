
/** Backend event subscriptions. */
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, useState } from 'react';
import type { ScanProgress } from '../types';

/** Fired while Envision scans and fingerprints, and once when it finishes. */
export const SCAN_EVENT = 'scan:progress';

/** Fired when the watcher notices changes made outside the app. */
export const CHANGED_EVENT = 'library:changed';

export interface LibraryChangedEvent {
  library_id: string;
  paths: string[];
}

/** Return the latest scan event and unsubscribe when the component unmounts. */
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
      // Running outside the desktop shell; there is nothing to listen to.
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  return progress;
}

/** Subscribe to out-of-band filesystem changes for the open library. */
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