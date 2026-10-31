import { describe, expect, it } from 'vitest';
import {
  CUSTOM_TOKENS,
  applyTheme,
  contrastRatio,
  deriveCustomTokens,
  isThemeMode,
  mix,
  normalizeHex,
  parseHex,
  readableOn,
  relativeLuminance,
  resolveTheme,
  type CustomPalette,
} from './theme';

const PALETTE: CustomPalette = {
  text: '#f5f5f5',
  textMuted: '#a0a0a0',
  accent: '#ff8800',
  background: '#101012',
};

describe('normalizeHex', () => {
  it('accepts six hex digits in any case and normalises them', () => {
    expect(normalizeHex('#A1B2C3', '#000000')).toBe('#a1b2c3');
    expect(normalizeHex('a1b2c3', '#000000')).toBe('#a1b2c3');
    expect(normalizeHex('  #a1b2c3  ', '#000000')).toBe('#a1b2c3');
  });

  it('falls back rather than passing something that is not a colour to CSS', () => {
    for (const bad of ['', 'nope', '#12', '#1234567', 'rgb(1,2,3)', 'red']) {
      expect(normalizeHex(bad, '#123456')).toBe('#123456');
    }
  });

  it('rejects three-digit shorthand', () => {
    expect(normalizeHex('#f00', '#123456')).toBe('#123456');
  });
});

describe('colour maths', () => {
  it('parses and round-trips channels', () => {
    expect(parseHex('#ff8800')).toEqual([255, 136, 0]);
    expect(parseHex('not a colour')).toBeNull();
  });

  it('mixes towards a target and clamps the amount', () => {
    expect(mix('#000000', '#ffffff', 0)).toBe('#000000');
    expect(mix('#000000', '#ffffff', 1)).toBe('#ffffff');
    expect(mix('#000000', '#ffffff', 0.5)).toBe('#808080');
    expect(mix('#000000', '#ffffff', 4)).toBe('#ffffff');
  });

  it('measures luminance and contrast the WCAG way', () => {
    expect(relativeLuminance('#000000')).toBeCloseTo(0, 5);
    expect(relativeLuminance('#ffffff')).toBeCloseTo(1, 5);
    expect(contrastRatio('#000000', '#ffffff')).toBeCloseTo(21, 5);
    expect(contrastRatio('#123456', '#123456')).toBeCloseTo(1, 5);
  });

  it('picks text that is legible on an accent colour', () => {
    expect(readableOn('#ffe066')).toBe('#0b0b0d');
    expect(readableOn('#1a237e')).toBe('#ffffff');
  });
});

describe('resolveTheme', () => {
  it('only consults the OS preference for system', () => {
    expect(resolveTheme('system', true)).toBe('grey');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('grey', false)).toBe('grey');
    expect(resolveTheme('oled', false)).toBe('oled');
    expect(resolveTheme('custom', true)).toBe('custom');
  });

  it('recognises only the modes it can render', () => {
    expect(isThemeMode('oled')).toBe(true);
    expect(isThemeMode('dark')).toBe(false);
    expect(isThemeMode('')).toBe(false);
  });
});

describe('deriveCustomTokens', () => {
  it('shades surfaces away from a dark background', () => {
    const tokens = deriveCustomTokens(PALETTE);

    expect(tokens['--bg']).toBe('#101012');
    expect(relativeLuminance(tokens['--surface'])).toBeGreaterThan(
      relativeLuminance(tokens['--bg']),
    );
    expect(relativeLuminance(tokens['--surface-2'])).toBeGreaterThan(
      relativeLuminance(tokens['--surface']),
    );
    expect(tokens['color-scheme']).toBe('dark');
  });

  it('lifts panes towards white on a light background', () => {
    const tokens = deriveCustomTokens({ ...PALETTE, background: '#f6f6f4' });

    expect(relativeLuminance(tokens['--surface'])).toBeGreaterThan(
      relativeLuminance(tokens['--bg']),
    );
    expect(relativeLuminance(tokens['--surface-2'])).toBeLessThan(
      relativeLuminance(tokens['--bg']),
    );
    expect(tokens['color-scheme']).toBe('light');
    expect(tokens['--danger']).toBe('#b91c1c');
  });

  it('keeps the four colours the user chose', () => {
    const tokens = deriveCustomTokens(PALETTE);

    expect(tokens['--text']).toBe('#f5f5f5');
    expect(tokens['--text-muted']).toBe('#a0a0a0');
    expect(tokens['--accent']).toBe('#ff8800');
    expect(tokens['--accent-contrast']).toBe(readableOn('#ff8800'));
  });

  it('sets exactly the properties applyTheme knows how to clear', () => {
    expect(Object.keys(deriveCustomTokens(PALETTE)).sort()).toEqual([...CUSTOM_TOKENS].sort());
  });

  it('cannot emit a value that is not a colour', () => {
    const tokens = deriveCustomTokens({
      text: 'red',
      textMuted: '',
      accent: '#12345',
      background: 'chartreuse',
    });

    for (const property of ['--bg', '--surface', '--text', '--text-muted', '--accent']) {
      expect(tokens[property]).toMatch(/^#[0-9a-f]{6}$/);
    }
  });
});

function fakeRoot() {
  const classes = new Set<string>();
  const properties = new Map<string, string>();
  return {
    classList: {
      toggle: (name: string, force: boolean) => {
        if (force) classes.add(name);
        else classes.delete(name);
      },
    },
    style: {
      setProperty: (name: string, value: string) => void properties.set(name, value),
      removeProperty: (name: string) => void properties.delete(name),
    },
    classes,
    properties,
  };
}

describe('applyTheme', () => {
  it('applies exactly one palette class', () => {
    const root = fakeRoot();

    applyTheme(root as unknown as HTMLElement, {
      mode: 'custom',
      prefersDark: false,
      palette: PALETTE,
      backgroundUrl: null,
      backgroundBlur: 0,
    });
    expect([...root.classes]).toEqual(['theme-custom']);

    applyTheme(root as unknown as HTMLElement, {
      mode: 'oled',
      prefersDark: false,
      palette: PALETTE,
      backgroundUrl: null,
      backgroundBlur: 0,
    });
    expect([...root.classes]).toEqual(['theme-oled']);
  });

  it('clears every custom token when a fixed palette takes over', () => {
    const root = fakeRoot();

    applyTheme(root as unknown as HTMLElement, {
      mode: 'custom',
      prefersDark: false,
      palette: PALETTE,
      backgroundUrl: null,
      backgroundBlur: 0,
    });
    expect(root.properties.get('--text')).toBe('#f5f5f5');

    applyTheme(root as unknown as HTMLElement, {
      mode: 'grey',
      prefersDark: false,
      palette: PALETTE,
      backgroundUrl: null,
      backgroundBlur: 0,
    });
    for (const property of CUSTOM_TOKENS) {
      expect(root.properties.has(property)).toBe(false);
    }
  });

  it('publishes the background image and its blur only while one is set', () => {
    const root = fakeRoot();

    applyTheme(root as unknown as HTMLElement, {
      mode: 'custom',
      prefersDark: false,
      palette: PALETTE,
      backgroundUrl: 'blob:http://localhost:1420/abc',
      backgroundBlur: 18,
    });
    expect(root.properties.get('--theme-background-image')).toBe(
      'url("blob:http://localhost:1420/abc")',
    );
    expect(root.properties.get('--theme-background-blur')).toBe('18px');

    applyTheme(root as unknown as HTMLElement, {
      mode: 'custom',
      prefersDark: false,
      palette: PALETTE,
      backgroundUrl: null,
      backgroundBlur: 18,
    });
    expect(root.properties.has('--theme-background-image')).toBe(false);
    expect(root.properties.has('--theme-background-blur')).toBe(false);
  });

  it('follows the OS when set to system', () => {
    const root = fakeRoot();

    applyTheme(root as unknown as HTMLElement, {
      mode: 'system',
      prefersDark: true,
      palette: PALETTE,
      backgroundUrl: null,
      backgroundBlur: 0,
    });
    expect([...root.classes]).toEqual(['theme-grey']);
  });
});
