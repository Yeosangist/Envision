import { useCallback, useEffect, useRef, useState } from 'react';
import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
import { CommandError, type Stats } from '../../types';
import { checkGalleryDl, libraryStats, pruneAllThumbnails } from '../../api/invoke';
import { CloseIcon } from '../../components/icons';
import { useLibraryStore } from '../../state/libraryStore';
import { useSettingsStore, type ThemeMode } from '../../state/settingsStore';
import { THEME_MODES, isThemeMode } from './theme';
import { thumbnailCpuOptions } from './cpu';
import { formatBytes, formatDuration } from './format';

/** Debounce streamed controls to avoid a database write for every pointer event. */
const COMMIT_DELAY_MS = 150;

const THEME_LABELS: Record<ThemeMode, { label: string; hint: string }> = {
  system: { label: 'System', hint: 'Follows the desktop’s light or dark setting.' },
  light: { label: 'Light', hint: 'Bright surfaces and dark text.' },
  grey: { label: 'Grey', hint: 'The soft dark palette Envision has always used.' },
  oled: { label: 'OLED', hint: 'Black surfaces, for an OLED display.' },
  custom: {
    label: 'Custom',
    hint: 'Your own text and accent colours, over a colour or a picture.',
  },
};

type Tab = 'appearance' | 'import' | 'stats';

const TABS: { id: Tab; label: string }[] = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'import', label: 'Import' },
  { id: 'stats', label: 'Stats' },
];

export interface SettingsModalProps {
  onClose: () => void;
  /** Milliseconds from page load to the app's first render. */
  readyMs: number;
}

export function SettingsModal({ onClose, readyMs }: SettingsModalProps) {
  const [tab, setTab] = useState<Tab>('appearance');

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  return (
    <>
      <button
        type="button"
        className="modal-scrim"
        aria-label="Close settings"
        onClick={onClose}
      />

      <div className="modal" role="dialog" aria-modal="true" aria-label="Settings">
        <header className="modal-heading">
          <h2>Settings</h2>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            aria-label="Close settings"
            title="Close"
          >
            <CloseIcon />
          </button>
        </header>

        <div className="modal-tabs" role="tablist" aria-label="Settings sections">
          {TABS.map(({ id, label }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={tab === id}
              className={tab === id ? 'is-active' : ''}
              onClick={() => setTab(id)}
            >
              {label}
            </button>
          ))}
        </div>

        <div className="modal-body">
          {tab === 'appearance' ? (
            <AppearanceTab />
          ) : tab === 'import' ? (
            <ImportTab />
          ) : (
            <StatsTab readyMs={readyMs} />
          )}
        </div>
      </div>
    </>
  );
}

function AppearanceTab() {
  const theme = useSettingsStore((state) => state.theme);
  const customText = useSettingsStore((state) => state.custom_text);
  const customTextMuted = useSettingsStore((state) => state.custom_text_muted);
  const customAccent = useSettingsStore((state) => state.custom_accent);
  const customBackground = useSettingsStore((state) => state.custom_background);
  const backgroundImage = useSettingsStore((state) => state.custom_background_image);
  const backgroundBlur = useSettingsStore((state) => state.custom_background_blur);
  const hideDuplicates = useSettingsStore((state) => state.hide_duplicates);
  const backgroundUrl = useSettingsStore((state) => state.backgroundUrl);
  const error = useSettingsStore((state) => state.error);

  const setTheme = useSettingsStore((state) => state.setTheme);
  const set = useSettingsStore((state) => state.set);
  const chooseBackground = useSettingsStore((state) => state.chooseBackground);
  const dismissError = useSettingsStore((state) => state.dismissError);

  const [choosing, setChoosing] = useState(false);

  const mode: ThemeMode = isThemeMode(theme) ? theme : 'system';

  const handleChooseImage = async () => {
    setChoosing(true);
    try {
      const picked = await openFileDialog({
        multiple: false,
        directory: false,
        title: 'Choose a background image',
      });
      if (typeof picked !== 'string') return;
      await chooseBackground(picked);
    } finally {
      setChoosing(false);
    }
  };

  return (
    <div className="settings-pane">
      <section className="settings-group">
        <h3>Theme</h3>
        <div className="settings-choices" role="radiogroup" aria-label="Theme">
          {THEME_MODES.map((option) => (
            <button
              key={option}
              type="button"
              role="radio"
              aria-checked={mode === option}
              className={mode === option ? 'is-active' : ''}
              title={THEME_LABELS[option].hint}
              onClick={() => void setTheme(option)}
            >
              {THEME_LABELS[option].label}
            </button>
          ))}
        </div>
        <p className="settings-hint">{THEME_LABELS[mode].hint}</p>
      </section>

      {mode === 'custom' && (
        <section className="settings-group">
          <h3>Custom colours</h3>
          <div className="settings-colours">
            <ColourField
              label="Main text"
              value={customText}
              onChange={(value) => void set({ custom_text: value })}
            />
            <ColourField
              label="Muted text"
              value={customTextMuted}
              onChange={(value) => void set({ custom_text_muted: value })}
            />
            <ColourField
              label="Accent"
              value={customAccent}
              onChange={(value) => void set({ custom_accent: value })}
            />
            <ColourField
              label="Background"
              value={customBackground}
              onChange={(value) => void set({ custom_background: value })}
            />
          </div>
          <p className="settings-hint">
            Bars, panels and borders are shaded from the background colour, so the two cannot
            drift apart. Icons and selected tiles use the accent.
          </p>

          <div className="settings-field">
            <span className="settings-field-label">Background image</span>
            {backgroundImage ? (
              <div className="settings-image">
                {/* Use a background image so a missing preview does not show a broken-image icon. */}
                <span
                  className="settings-image-preview"
                  style={backgroundUrl ? { backgroundImage: `url("${backgroundUrl}")` } : undefined}
                  aria-hidden="true"
                />
                <span className="settings-image-name" title={backgroundImage}>
                  {backgroundImage}
                </span>
                <button type="button" onClick={() => void chooseBackground(null)}>
                  Remove
                </button>
              </div>
            ) : (
              <button type="button" onClick={() => void handleChooseImage()} disabled={choosing}>
                {choosing ? 'Choosing…' : 'Choose an image…'}
              </button>
            )}
          </div>

          {backgroundImage && (
            <div className="settings-field">
              <span className="settings-field-label">Background blur</span>
              <BlurSlider
                value={backgroundBlur}
                onChange={(value) => void set({ custom_background_blur: value })}
              />
            </div>
          )}

          <p className="settings-hint">
            The picture is read from where it lives every time Envision starts. Moving or
            deleting it leaves the theme without a backdrop.
          </p>
        </section>
      )}

      {error && (
        <p className="settings-error" role="alert">
          {error}
          <button type="button" onClick={dismissError} aria-label="Dismiss">
            ✕
          </button>
        </p>
      )}

      <section className="settings-group">
        <h3>Browsing</h3>
        <label className="settings-toggle">
          <input
            type="checkbox"
            checked={hideDuplicates}
            onChange={(event) => void set({ hide_duplicates: event.target.checked })}
          />
          <span>
            <span className="settings-toggle-title">Hide duplicates</span>
            <span className="settings-hint">
              Removes the Duplicates and Browse buttons, the count badge and the ×N badges on
              tiles. Identical files are still detected — nothing is moved, renamed or deleted
              to resolve one, and two copies in two folders stay two copies.
            </span>
          </span>
        </label>
      </section>
    </div>
  );
}

/** Colour input that debounces streamed picker values. */
function ColourField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const [local, setLocal] = useState(value);
  const commit = useDeferredCommit(onChange);

  // Sync local input state after theme changes or backend normalization.
  useEffect(() => {
    setLocal(value);
  }, [value]);

  return (
    <label className="settings-colour">
      <input
        type="color"
        value={local}
        aria-label={label}
        onChange={(event) => {
          setLocal(event.target.value);
          commit(event.target.value);
        }}
      />
      <span>{label}</span>
    </label>
  );
}

function BlurSlider({ value, onChange }: { value: number; onChange: (value: number) => void }) {
  const [local, setLocal] = useState(value);
  const commit = useDeferredCommit(onChange);

  useEffect(() => {
    setLocal(value);
  }, [value]);

  return (
    <span className="settings-slider">
      <input
        type="range"
        min={0}
        max={40}
        step={1}
        value={Math.round(local)}
        aria-label="Background blur"
        onChange={(event) => {
          const next = Number(event.target.value);
          setLocal(next);
          commit(next);
        }}
      />
      <span className="settings-field-value">{Math.round(local)} px</span>
    </span>
  );
}

function useDeferredCommit<T>(commit: (value: T) => void) {
  const timer = useRef<number | null>(null);
  const latest = useRef(commit);
  latest.current = commit;

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  return useCallback(
    (value: T) => {
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(() => latest.current(value), COMMIT_DELAY_MS);
    },
    [],
  );
}

function ImportTab() {
  const importFolder = useSettingsStore((state) => state.import_folder);
  const importAsk = useSettingsStore((state) => state.import_ask);
  const thumbnailCpuLimit = useSettingsStore((state) => state.thumbnail_cpu_limit);
  const set = useSettingsStore((state) => state.set);
  const [status, setStatus] = useState<'checking' | 'found' | 'missing'>('checking');
  const cpuOptions = thumbnailCpuOptions(
    thumbnailCpuLimit,
    navigator.hardwareConcurrency ?? 1,
  );

  useEffect(() => {
    let cancelled = false;
    void checkGalleryDl().then((found) => {
      if (cancelled) return;
      setStatus(found ? 'found' : 'missing');
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const handleChoose = async () => {
    const picked = await openFileDialog({ directory: true, multiple: false, title: 'Default import folder' });
    if (typeof picked !== 'string') return;
    await set({ import_folder: picked });
  };

  return (
    <div className="settings-pane">
      <section className="settings-group">
        <h3>gallery-dl</h3>
        <div className="settings-field">
          <span className="settings-field-label">System check</span>
          <span className={status === 'found' ? 'settings-found' : status === 'missing' ? 'settings-missing' : ''}>
            {status === 'checking' ? 'Checking…' : status === 'found' ? 'Found!' : 'Missing?'}
          </span>
        </div>
        <p className="settings-hint">
          gallery-dl does the actual downloading. Envision invokes it from your PATH; install it
          separately if it is reported missing.
        </p>
      </section>

      <section className="settings-group">
        <h3>Destination</h3>
        <div className="settings-field">
          <span className="settings-field-label">Default folder</span>
          {importFolder ? (
            <div className="settings-image">
              <span className="settings-image-name" title={importFolder}>
                {importFolder}
              </span>
              <button type="button" onClick={() => void set({ import_folder: null })}>
                Clear
              </button>
            </div>
          ) : (
            <button type="button" onClick={() => void handleChoose()}>
              Choose a folder…
            </button>
          )}
        </div>
        <label className="settings-toggle">
          <input
            type="checkbox"
            checked={importAsk}
            onChange={(event) => void set({ import_ask: event.target.checked })}
          />
          <span>
            <span className="settings-toggle-title">Ask every time</span>
            <span className="settings-hint">
              When unticked and a default folder is set, imports go there without prompting.
            </span>
          </span>
        </label>
      </section>

      <section className="settings-group">
        <h3>Performance</h3>
        <div className="settings-field">
          <span className="settings-field-label">Thumbnail CPU limit</span>
          <select
            aria-label="Thumbnail CPU limit"
            value={thumbnailCpuLimit}
            onChange={(event) =>
              void set({ thumbnail_cpu_limit: Number(event.target.value) })
            }
          >
            <option value={0}>All available cores</option>
            {cpuOptions.map((cores) => (
              <option key={cores} value={cores}>
                {cores} {cores === 1 ? 'core' : 'cores'}
              </option>
            ))}
          </select>
        </div>
        <p className="settings-hint">
          Caps the threads used while building thumbnails, across every batch the
          app starts. A lower limit keeps the first scan of a large library from
          saturating the machine. Scanning and hashing use their own pool and are
          not affected; a change applies to thumbnail work started after it.
        </p>
      </section>
    </div>
  );
}

function StatsTab({ readyMs }: { readyMs: number }) {
  const [stats, setStats] = useState<Stats | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [reading, setReading] = useState(false);
  const [pruning, setPruning] = useState(false);
  const [pruned, setPruned] = useState<number | null>(null);

  const lastOpenMs = useLibraryStore((state) => state.lastOpenMs);
  const hideDuplicates = useSettingsStore((state) => state.hide_duplicates);

  const refresh = useCallback(async () => {
    setReading(true);
    try {
      setStats(await libraryStats());
      setFailure(null);
    } catch (error) {
      setFailure(CommandError.from(error).message);
    } finally {
      setReading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const tracked = [
    { label: 'Libraries', value: (stats?.libraries ?? 0).toLocaleString() },
    {
      label: 'Images',
      value: (stats?.images ?? 0).toLocaleString(),
      hint: 'across every library Envision tracks',
    },
    {
      label: 'Folders',
      value: (stats?.folders ?? 0).toLocaleString(),
      hint: 'sub-folders, not counting each root',
    },
  ];

  if (!hideDuplicates) {
    tracked.push({
      label: 'Duplicates',
      value: (stats?.duplicate_groups ?? 0).toLocaleString(),
      hint: `${(stats?.redundant ?? 0).toLocaleString()} extra copies`,
    });
  }

  return (
    <div className="settings-pane">
      <section className="settings-group">
        <div className="settings-group-heading">
          <h3>Tracked</h3>
          <button type="button" onClick={() => void refresh()} disabled={reading}>
            {reading ? 'Reading…' : 'Refresh'}
          </button>
        </div>
        <div className="settings-stats">
          {tracked.map((card) => (
            <Stat key={card.label} {...card} />
          ))}
        </div>
      </section>

      <section className="settings-group">
        <h3>On disk</h3>
        <div className="settings-stats">
          <Stat
            label="Images"
            value={formatBytes(stats?.library_bytes ?? 0)}
            hint="as recorded when indexed"
          />
          <Stat label="Thumbnails" value={formatBytes(stats?.thumbnails_bytes ?? 0)} />
          <Stat label="Index" value={formatBytes(stats?.index_bytes ?? 0)} />
        </div>
        <div className="settings-field">
          <span className="settings-field-label">Cache location</span>
          <code className="settings-path">{stats?.cache_location ?? '—'}</code>
        </div>
        <div className="settings-field">
          <button
            type="button"
            disabled={pruning}
            onClick={async () => {
              setPruning(true);
              try {
                const removed = await pruneAllThumbnails();
                setPruned(removed);
                await refresh();
              } catch (error) {
                setFailure(CommandError.from(error).message);
              } finally {
                setPruning(false);
              }
            }}
          >
            {pruning ? 'Pruning…' : 'Prune thumbnail cache'}
          </button>
          {pruned !== null && (
            <span className="settings-hint">{pruned} file{pruned === 1 ? '' : 's'} removed</span>
          )}
        </div>
        <p className="settings-hint">
          Application data, never inside a library. Forgetting a library removes its index and
          its thumbnails; your images are left exactly where they are.
        </p>
      </section>

      <section className="settings-group">
        <h3>This session</h3>
        <div className="settings-stats">
          <Stat label="UI ready" value={formatDuration(readyMs)} />
          <Stat
            label="Last library opened"
            value={formatDuration(lastOpenMs)}
            hint="includes the first scan of a new library"
          />
        </div>
      </section>

      {failure && (
        <p className="settings-error" role="alert">
          {failure}
        </p>
      )}

      {stats && stats.unreadable > 0 && (
        <p className="settings-hint" role="status">
          {stats.unreadable} {stats.unreadable === 1 ? 'library' : 'libraries'} could not be
          read, so the totals above are incomplete.
        </p>
      )}
    </div>
  );
}

function Stat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="settings-stat">
      <span className="settings-stat-value">{value}</span>
      <span className="settings-stat-label">{label}</span>
      {hint && <span className="settings-stat-hint">{hint}</span>}
    </div>
  );
}
