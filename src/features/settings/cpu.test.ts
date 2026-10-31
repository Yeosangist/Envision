import { describe, expect, it } from 'vitest';
import { MAX_THUMBNAIL_CPU_LIMIT, thumbnailCpuOptions } from './cpu';

describe('thumbnailCpuOptions', () => {
  it('offers one option per available core', () => {
    expect(thumbnailCpuOptions(0, 4)).toEqual([1, 2, 3, 4]);
  });

  it('keeps a stored limit that is higher than this machine', () => {
    // Selects render blank when the value has no matching option, so a value
    // carried over from a larger machine still has to be offered.
    expect(thumbnailCpuOptions(8, 4)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it('caps options at the backend maximum', () => {
    const options = thumbnailCpuOptions(0, 128);

    expect(options).toHaveLength(MAX_THUMBNAIL_CPU_LIMIT);
    expect(options[options.length - 1]).toBe(MAX_THUMBNAIL_CPU_LIMIT);
  });

  it('treats nonsense cores and limits as sane numbers', () => {
    expect(thumbnailCpuOptions(-4, 4)).toEqual([1, 2, 3, 4]);
    expect(thumbnailCpuOptions(Number.NaN, 2.9)).toEqual([1, 2]);
    expect(thumbnailCpuOptions(0, 0)).toEqual([1]);
  });
});
