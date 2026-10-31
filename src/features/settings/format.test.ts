import { describe, expect, it } from 'vitest';
import { formatBytes, formatDuration } from './format';

describe('formatBytes', () => {
  it('keeps whole bytes whole and steps up at 1024', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1)).toBe('1 B');
    expect(formatBytes(1023)).toBe('1023 B');
    expect(formatBytes(1024)).toBe('1.0 kB');
    expect(formatBytes(1536)).toBe('1.5 kB');
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB');
    expect(formatBytes(1024 ** 3 * 2.5)).toBe('2.5 GB');
  });

  it('does not step past its largest unit', () => {
    expect(formatBytes(1024 ** 5)).toBe('1024.0 TB');
  });

  it('treats nonsense as nothing rather than printing it', () => {
    expect(formatBytes(-1)).toBe('0 B');
    expect(formatBytes(Number.NaN)).toBe('0 B');
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe('0 B');
  });
});

describe('formatDuration', () => {
  it('uses milliseconds, then seconds, then minutes', () => {
    expect(formatDuration(0)).toBe('0 ms');
    expect(formatDuration(999)).toBe('999 ms');
    expect(formatDuration(1000)).toBe('1.0 s');
    expect(formatDuration(2500)).toBe('2.5 s');
    expect(formatDuration(60_000)).toBe('1 min 0 s');
    expect(formatDuration(95_500)).toBe('1 min 36 s');
  });

  it('shows an em dash when there is nothing measured yet', () => {
    expect(formatDuration(null)).toBe('—');
    expect(formatDuration(-5)).toBe('—');
  });
});
