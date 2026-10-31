
export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 B';

  const units = ['B', 'kB', 'MB', 'GB', 'TB'];
  let value = bytes;
  let unit = 0;

  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

export function formatDuration(ms: number | null): string {
  if (ms === null || !Number.isFinite(ms) || ms < 0) return '—';
  if (ms < 1000) return `${Math.round(ms)} ms`;

  const seconds = ms / 1000;
  if (seconds < 60) return `${seconds.toFixed(1)} s`;

  const minutes = Math.floor(seconds / 60);
  return `${minutes} min ${Math.round(seconds - minutes * 60)} s`;
}
