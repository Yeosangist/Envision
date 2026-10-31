/** Maximum supported CPU limit, matching Rust settings normalization. */
export const MAX_THUMBNAIL_CPU_LIMIT = 32;

function clampInteger(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(max, Math.max(min, Math.trunc(value)));
}

/** Include the stored limit even when it exceeds this machine's reported cores. */
export function thumbnailCpuOptions(limit: number, available: number): number[] {
  const cores = clampInteger(available, 1, MAX_THUMBNAIL_CPU_LIMIT);
  const current = clampInteger(limit, 0, MAX_THUMBNAIL_CPU_LIMIT);
  const highest = Math.max(cores, current);
  return Array.from({ length: highest }, (_, index) => index + 1);
}
