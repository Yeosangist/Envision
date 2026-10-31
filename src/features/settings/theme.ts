
export type ThemeMode = 'system' | 'light' | 'grey' | 'oled' | 'custom';

/** A theme mode with `system` resolved against the OS preference. */
export type ResolvedTheme = 'light' | 'grey' | 'oled' | 'custom';

export const THEME_MODES: readonly ThemeMode[] = ['system', 'light', 'grey', 'oled', 'custom'];

export interface CustomPalette {
  text: string;
  textMuted: string;
  accent: string;
  background: string;
}

/** Class names applied to the document element, one per fixed palette. */
export const THEME_CLASSES = ['theme-light', 'theme-grey', 'theme-oled', 'theme-custom'] as const;

/** Inline tokens removed when switching from a custom palette to a fixed theme. */
export const CUSTOM_TOKENS = [
  '--bg',
  '--surface',
  '--surface-2',
  '--border',
  '--text',
  '--text-muted',
  '--accent',
  '--accent-contrast',
  '--danger',
  '--overlay',
  '--shadow',
  'color-scheme',
] as const;

/** Luminance threshold for shading custom surfaces lighter or darker. */
const DARK_BACKGROUND_LUMINANCE = 0.4;

const OVERLAY_DARK = 'rgba(0, 0, 0, 0.88)';
const OVERLAY_LIGHT = 'rgba(0, 0, 0, 0.72)';
const SHADOW_DARK = '0 1px 2px rgba(0, 0, 0, 0.4), 0 8px 24px rgba(0, 0, 0, 0.35)';
const SHADOW_LIGHT = '0 1px 2px rgba(0, 0, 0, 0.06), 0 8px 24px rgba(0, 0, 0, 0.06)';

export function isThemeMode(value: string): value is ThemeMode {
  return (THEME_MODES as readonly string[]).includes(value);
}

/** Normalise a `#rrggbb` colour, falling back when it is anything else. */
export function normalizeHex(value: string, fallback: string): string {
  const trimmed = value.trim();
  const digits = trimmed.startsWith('#') ? trimmed.slice(1) : trimmed;
  return /^[0-9a-f]{6}$/i.test(digits) ? `#${digits.toLowerCase()}` : fallback;
}

export function parseHex(value: string): [number, number, number] | null {
  const normalized = normalizeHex(value, '');
  if (!normalized) return null;

  const int = Number.parseInt(normalized.slice(1), 16);
  return [(int >> 16) & 0xff, (int >> 8) & 0xff, int & 0xff];
}

export function toHex([r, g, b]: [number, number, number]): string {
  const channel = (value: number) =>
    Math.max(0, Math.min(255, Math.round(value))).toString(16).padStart(2, '0');
  return `#${channel(r)}${channel(g)}${channel(b)}`;
}

/** Blend `amount` of `to` into `from`. Zero returns `from`, one returns `to`. */
export function mix(from: string, to: string, amount: number): string {
  const a = parseHex(from) ?? [0, 0, 0];
  const b = parseHex(to) ?? [0, 0, 0];
  const t = Math.max(0, Math.min(1, amount));
  return toHex([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]);
}

function channelToLinear(value: number): number {
  const scaled = value / 255;
  return scaled <= 0.03928 ? scaled / 12.92 : ((scaled + 0.055) / 1.055) ** 2.4;
}

/** WCAG relative luminance, 0 for black and 1 for white. */
export function relativeLuminance(value: string): number {
  const [r, g, b] = parseHex(value) ?? [0, 0, 0];
  return (
    0.2126 * channelToLinear(r) + 0.7152 * channelToLinear(g) + 0.0722 * channelToLinear(b)
  );
}

/** WCAG contrast ratio, from 1 (none) to 21 (black on white). */
export function contrastRatio(a: string, b: string): number {
  const first = relativeLuminance(a);
  const second = relativeLuminance(b);
  const [lighter, darker] = first > second ? [first, second] : [second, first];
  return (lighter + 0.05) / (darker + 0.05);
}

/** Choose black or white text for readable contrast on the background. */
export function readableOn(background: string): string {
  return contrastRatio(background, '#ffffff') >= contrastRatio(background, '#0b0b0d')
    ? '#ffffff'
    : '#0b0b0d';
}

export function resolveTheme(mode: ThemeMode, prefersDark: boolean): ResolvedTheme {
  if (mode === 'system') return prefersDark ? 'grey' : 'light';
  return mode;
}

/** Derive remaining surface and contrast tokens from the four user-selected colors. */
export function deriveCustomTokens(palette: CustomPalette): Record<string, string> {
  const background = normalizeHex(palette.background, '#101012');
  const dark = relativeLuminance(background) < DARK_BACKGROUND_LUMINANCE;
  const accent = normalizeHex(palette.accent, '#60a5fa');

  return {
    '--bg': background,
    '--surface': dark ? mix(background, '#ffffff', 0.05) : mix(background, '#ffffff', 0.7),
    '--surface-2': dark ? mix(background, '#ffffff', 0.1) : mix(background, '#000000', 0.05),
    '--border': dark ? mix(background, '#ffffff', 0.16) : mix(background, '#000000', 0.14),
    '--text': normalizeHex(palette.text, '#ececed'),
    '--text-muted': normalizeHex(palette.textMuted, '#9a9aa2'),
    '--accent': accent,
    '--accent-contrast': readableOn(accent),
    '--danger': dark ? '#f87171' : '#b91c1c',
    '--overlay': dark ? OVERLAY_DARK : OVERLAY_LIGHT,
    '--shadow': dark ? SHADOW_DARK : SHADOW_LIGHT,
    'color-scheme': dark ? 'dark' : 'light',
  };
}

export interface ThemeRequest {
  mode: ThemeMode;
  /** The OS preference, only consulted when the mode is `system`. */
  prefersDark: boolean;
  palette: CustomPalette;
  /** Blob URL of the chosen background image, if there is one. */
  backgroundUrl: string | null;
  backgroundBlur: number;
}

/** Apply theme classes and tokens, removing custom inline values for fixed palettes. */
export function applyTheme(root: HTMLElement, request: ThemeRequest): void {
  const resolved = resolveTheme(request.mode, request.prefersDark);

  for (const name of THEME_CLASSES) {
    root.classList.toggle(name, name === `theme-${resolved}`);
  }

  const tokens = resolved === 'custom' ? deriveCustomTokens(request.palette) : null;
  for (const property of CUSTOM_TOKENS) {
    if (tokens) {
      root.style.setProperty(property, tokens[property]);
    } else {
      root.style.removeProperty(property);
    }
  }

  if (request.backgroundUrl) {
    root.style.setProperty('--theme-background-image', `url("${request.backgroundUrl}")`);
    root.style.setProperty(
      '--theme-background-blur',
      `${Math.max(0, request.backgroundBlur)}px`,
    );
  } else {
    root.style.removeProperty('--theme-background-image');
    root.style.removeProperty('--theme-background-blur');
  }
}
