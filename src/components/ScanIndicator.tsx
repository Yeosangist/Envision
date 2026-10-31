import { useEffect, useState } from 'react';
import type { ScanProgress } from '../types';

/** How long a finished scan stays up before getting out of the way. */
const DISMISS_AFTER_MS = 4000;

const isTerminal = (phase: ScanProgress['phase']) =>
  phase === 'done' || phase === 'cancelled' || phase === 'failed';

/** Shows scan progress and dismisses terminal states after a short delay. */
export function ScanIndicator({ progress }: { progress: ScanProgress | null }) {
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    if (!progress) {
      setDismissed(false);
      return undefined;
    }

    if (!isTerminal(progress.phase)) {
      setDismissed(false);
      return undefined;
    }

    const timer = window.setTimeout(() => setDismissed(true), DISMISS_AFTER_MS);
    return () => window.clearTimeout(timer);
  }, [progress]);

  if (!progress || dismissed) return null;

  const percent =
    progress.total > 0 ? Math.min(100, Math.round((progress.processed / progress.total) * 100)) : 0;

  const label =
    progress.phase === 'scanning'
      ? 'Reading folders…'
      : progress.phase === 'fingerprinting'
        ? `Checking for duplicates — ${progress.processed.toLocaleString()} of ${progress.total.toLocaleString()}`
        : progress.phase === 'cancelled'
          ? 'Scan stopped'
          : progress.phase === 'failed'
            ? 'Scan could not finish — browse the library as it is'
            : `Ready — ${progress.duplicates.toLocaleString()} redundant cop${progress.duplicates === 1 ? 'y' : 'ies'}`;

  const failed = progress.phase === 'failed';

  return (
    <div className={`scan-indicator${failed ? ' is-failed' : ''}`} role="status">
      <div className="scan-indicator-track">
        <div
          className={`scan-indicator-fill${progress.phase === 'fingerprinting' ? ' is-indeterminate' : ''}`}
          style={progress.total > 0 ? { width: `${percent}%` } : undefined}
        />
      </div>
      <span className="scan-indicator-label">{label}</span>
      {progress.current && <span className="scan-indicator-file">{progress.current}</span>}
    </div>
  );
}