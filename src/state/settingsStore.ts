import { create } from 'zustand';
import {
  getSettings,
  readThemeBackground,
  setThemeBackground,
  updateSettings,
} from '../api/invoke';
import { mimeFor } from '../api/assets';
import { CommandError, type Settings } from '../types';
import {
  applyTheme,
  isThemeMode,
  type CustomPalette,
  type ThemeMode,
} from '../features/settings/theme';

export type { ThemeMode };

/** Temporary defaults, replaced by normalized backend settings on load. */
const DEFAULTS: Settings = {
  theme: 'grey',
  interface_scale: 1,
  gif_autoplay: true,
  max_playing_gifs: 4,
  folder_card_size: 160,
  hide_duplicates: false,
  thumbnail_cpu_limit: 0,
  custom_text: '#ececed',
  custom_text_muted: '#9a9aa2',
  custom_accent: '#60a5fa',
  custom_background: '#101012',
  custom_background_image: null,
  custom_background_blur: 12,
  import_folder: null,
  import_ask: true,
};

/** Per-machine desktop preferences. */
interface SettingsState extends Settings {
  loaded: boolean;
  /** Blob URL for the background image; its filesystem path stays in Rust. */
  backgroundUrl: string | null;
  /** A failure worth showing, such as a background image that has gone away. */
  error: string | null;
  apply: (root: HTMLElement) => void;
  load: () => Promise<void>;
  set: (patch: Partial<Settings>) => Promise<void>;
  setTheme: (theme: ThemeMode) => Promise<void>;
  chooseBackground: (path: string | null) => Promise<void>;
  dismissError: () => void;
}

/** The persisted fields alone, so store-only state never rides along to Rust. */
function persisted(state: SettingsState): Settings {
  return {
    theme: state.theme,
    interface_scale: state.interface_scale,
    gif_autoplay: state.gif_autoplay,
    max_playing_gifs: state.max_playing_gifs,
    folder_card_size: state.folder_card_size,
    hide_duplicates: state.hide_duplicates,
    thumbnail_cpu_limit: state.thumbnail_cpu_limit,
    custom_text: state.custom_text,
    custom_text_muted: state.custom_text_muted,
    custom_accent: state.custom_accent,
    custom_background: state.custom_background,
    custom_background_image: state.custom_background_image,
    custom_background_blur: state.custom_background_blur,
    import_folder: state.import_folder,
    import_ask: state.import_ask,
  };
}

function paletteOf(state: Settings): CustomPalette {
  return {
    text: state.custom_text,
    textMuted: state.custom_text_muted,
    accent: state.custom_accent,
    background: state.custom_background,
  };
}

/** Leave unknown image types unset so the browser can sniff their bytes. */
function backgroundBlob(bytes: ArrayBuffer, name: string): Blob {
  const dot = name.lastIndexOf('.');
  const type = mimeFor(dot === -1 ? '' : name.slice(dot + 1));
  const data = new Uint8Array(bytes);
  return type === 'application/octet-stream' ? new Blob([data]) : new Blob([data], { type });
}

export const useSettingsStore = create<SettingsState>((set, get) => ({
  ...DEFAULTS,
  loaded: false,
  backgroundUrl: null,
  error: null,

  apply: (root) => {
    const state = get();
    applyTheme(root, {
      mode: isThemeMode(state.theme) ? state.theme : 'system',
      prefersDark: window.matchMedia('(prefers-color-scheme: dark)').matches,
      palette: paletteOf(state),
      backgroundUrl: state.backgroundUrl,
      backgroundBlur: state.custom_background_blur,
    });
  },

  load: async () => {
    try {
      const settings = await getSettings();
      set({ ...settings, loaded: true });
    } catch {
      set({ loaded: true });
    }
    get().apply(document.documentElement);
    await refreshBackground(set, get);
  },

  set: async (patch) => {
    // Update immediately, then replace with backend-normalized settings.
    set(patch);
    get().apply(document.documentElement);
    try {
      const settings = await updateSettings({ ...persisted(get()), ...patch });
      set(settings);
      get().apply(document.documentElement);
    } catch {
    // Keep the in-memory value for this session if saving fails.
    }
  },

  setTheme: async (theme) => {
    await get().set({ theme });
  },

  chooseBackground: async (path) => {
    try {
      const name = await setThemeBackground(path);
      set({ custom_background_image: name, error: null });
      await refreshBackground(set, get);
    } catch (failure) {
      set({ error: failure instanceof CommandError ? failure.message : String(failure) });
    }
  },

  dismissError: () => set({ error: null }),
}));

/** Publish the stored background as a blob URL; ignore missing files so they do not block startup. */
async function refreshBackground(
  set: (partial: Partial<SettingsState>) => void,
  get: () => SettingsState,
): Promise<void> {
  const previous = get().backgroundUrl;
  if (previous) URL.revokeObjectURL(previous);

  if (!get().custom_background_image) {
    set({ backgroundUrl: null });
    get().apply(document.documentElement);
    return;
  }

  try {
    const bytes = await readThemeBackground();
    set({ backgroundUrl: URL.createObjectURL(backgroundBlob(bytes, get().custom_background_image ?? '')) });
  } catch {
    set({ backgroundUrl: null });
  }

  get().apply(document.documentElement);
}

/** React to the OS theme changing while the app is set to `system`. */
export function watchSystemTheme(): () => void {
  const media = window.matchMedia('(prefers-color-scheme: dark)');
  const handler = () => {
    if (useSettingsStore.getState().theme === 'system') {
      useSettingsStore.getState().apply(document.documentElement);
    }
  };
  media.addEventListener('change', handler);
  return () => media.removeEventListener('change', handler);
}
