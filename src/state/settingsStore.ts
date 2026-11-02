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
  interfaceScale: 1,
  gifAutoplay: true,
  maxPlayingGifs: 4,
  folderCardSize: 160,
  hideDuplicates: false,
  thumbnailCpuLimit: 0,
  customText: '#ececed',
  customTextMuted: '#9a9aa2',
  customAccent: '#60a5fa',
  customBackground: '#101012',
  customBackgroundImage: null,
  customBackgroundBlur: 12,
  importFolder: null,
  importAsk: true,
  importBrowserProfile: '',
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
    interfaceScale: state.interfaceScale,
    gifAutoplay: state.gifAutoplay,
    maxPlayingGifs: state.maxPlayingGifs,
    folderCardSize: state.folderCardSize,
    hideDuplicates: state.hideDuplicates,
    thumbnailCpuLimit: state.thumbnailCpuLimit,
    customText: state.customText,
    customTextMuted: state.customTextMuted,
    customAccent: state.customAccent,
    customBackground: state.customBackground,
    customBackgroundImage: state.customBackgroundImage,
    customBackgroundBlur: state.customBackgroundBlur,
    importFolder: state.importFolder,
    importAsk: state.importAsk,
    importBrowserProfile: state.importBrowserProfile,
  };
}

function paletteOf(state: Settings): CustomPalette {
  return {
    text: state.customText,
    textMuted: state.customTextMuted,
    accent: state.customAccent,
    background: state.customBackground,
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
      backgroundBlur: state.customBackgroundBlur,
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
      set({ customBackgroundImage: name, error: null });
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

  if (!get().customBackgroundImage) {
    set({ backgroundUrl: null });
    get().apply(document.documentElement);
    return;
  }

  try {
    const bytes = await readThemeBackground();
    set({ backgroundUrl: URL.createObjectURL(backgroundBlob(bytes, get().customBackgroundImage ?? '')) });
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
