
/** Formatting for the statistics screen. Pure, so the rounding is testable. */
/** A byte count in the unit that reads best, base 1024. */
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';

  const units = ['B', 'kB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }

  // Whole bytes never need a decimal; every larger unit reads better with one.
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

/** A duration in milliseconds, or an em dash when there is nothing to show. */
export function formatDuration(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms) || ms < 0) return '—';
  if (ms < 1000) return `${Math.round(ms)} ms`;

  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;

  const minutes = Math.floor(seconds / 60);
  return `${minutes} min ${Math.round(seconds - minutes * 60)} s`;
}
