import { useCallback, useEffect, useRef, useState } from 'react';
import { open as openFileDialog } from '@tauri-apps/plugin-dialog';
import { CommandError, type Stats } from '../../types';
import { checkGalleryDl, libraryStats, pruneAllThumbnails } from '../../api/invoke';
import {
  getBundleType,
  getIdentifier,
  getName,
  getTauriVersion,
  getVersion,
} from '@tauri-apps/api/app';
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
  grey: { label: 'Grey', hint: 'Default soft dark palette.' },
  oled: { label: 'OLED', hint: 'Black surfaces, for an OLED display.' },
  custom: {
    label: 'Custom',
    hint: 'Your own text and accent colours, over a colour or a picture.',
  },
};

type Tab = 'appearance' | 'import' | 'stats' | 'info';

const TABS: { id: Tab; label: string }[] = [
  { id: 'appearance', label: 'Appearance' },
  { id: 'import', label: 'Import' },
  { id: 'stats', label: 'Stats' },
  { id: 'info', label: 'Info' },
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
          ) : tab === 'stats' ? (
            <StatsTab readyMs={readyMs} />
          ) : tab === 'info' ? (
            <InfoTab />
          ) : null}
        </div>
      </div>
    </>
  );
}

function AppearanceTab() {
  const theme = useSettingsStore((state) => state.theme);
  const customText = useSettingsStore((state) => state.customText);
  const customTextMuted = useSettingsStore((state) => state.customTextMuted);
  const customAccent = useSettingsStore((state) => state.customAccent);
  const customBackground = useSettingsStore((state) => state.customBackground);
  const backgroundImage = useSettingsStore((state) => state.customBackgroundImage);
  const backgroundBlur = useSettingsStore((state) => state.customBackgroundBlur);
  const hideDuplicates = useSettingsStore((state) => state.hideDuplicates);
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
              onChange={(value) => void set({ customText: value })}
            />
            <ColourField
              label="Muted text"
              value={customTextMuted}
              onChange={(value) => void set({ customTextMuted: value })}
            />
            <ColourField
              label="Accent"
              value={customAccent}
              onChange={(value) => void set({ customAccent: value })}
            />
            <ColourField
              label="Background"
              value={customBackground}
              onChange={(value) => void set({ customBackground: value })}
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
                onChange={(value) => void set({ customBackgroundBlur: value })}
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
            onChange={(event) => void set({ hideDuplicates: event.target.checked })}
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
  const importFolder = useSettingsStore((state) => state.importFolder);
  const importAsk = useSettingsStore((state) => state.importAsk);
  const importBrowserProfile = useSettingsStore((state) => state.importBrowserProfile);
  const thumbnailCpuLimit = useSettingsStore((state) => state.thumbnailCpuLimit);
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
    await set({ importFolder: picked });
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
              <button type="button" onClick={() => void set({ importFolder: null })}>
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
            onChange={(event) => void set({ importAsk: event.target.checked })}
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
        <h3>Authentication</h3>
        <div className="settings-field">
          <span className="settings-field-label">Browser profile</span>
          <select
            aria-label="Browser profile"
            value={importBrowserProfile}
            onChange={(event) => void set({ importBrowserProfile: event.target.value })}
          >
            <option value="">None (public boards only)</option>
            <option value="brave">Brave</option>
            <option value="chrome">Chrome</option>
            <option value="chromium">Chromium</option>
            <option value="edge">Edge</option>
            <option value="firefox">Firefox</option>
            <option value="floorp">Floorp</option>
            <option value="librewolf">LibreWolf</option>
            <option value="opera">Opera</option>
            <option value="orion">Orion</option>
            <option value="safari">Safari</option>
            <option value="thorium">Thorium</option>
            <option value="vivaldi">Vivaldi</option>
            <option value="zen">Zen</option>
          </select>
        </div>
        <p className="settings-hint">
          gallery-dl reads the selected browser&apos;s default profile. Close the browser if its
          cookie database is locked.
        </p>
      </section>

      <section className="settings-group">
        <h3>Performance</h3>
        <div className="settings-field">
          <span className="settings-field-label">Thumbnail CPU limit</span>
          <select
            aria-label="Thumbnail CPU limit"
            value={thumbnailCpuLimit}
            onChange={(event) =>
              void set({ thumbnailCpuLimit: Number(event.target.value) })
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
  const hideDuplicates = useSettingsStore((state) => state.hideDuplicates);

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

const FAQ: { title: string; items: { q: string; a: string }[] }[] = [
  {
    title: 'Libraries & identity',
    items: [
      {
        q: "What does “forgetting” a library do?",
        a: "Removes Envision's index, thumbnails, and ordering for it. Your images are untouched. Re-adding the same folder reuses the existing index.",
      },
      {
        q: "I moved a library folder. Did I lose everything?",
        a: "No. The library is kept and flagged; use Relink to point it at the new location without losing order or fingerprints.",
      },
      {
        q: "Why does re-adding a folder sometimes feel instant?",
        a: "The library id is derived from the folder's canonical path, so a re-added folder reuses its existing index instead of starting over."
      },
      {
        q: "What happens if I rename a root folder outside of Envision?",
        a: "The library is flagged as broken. Use Relink to point it at the new location without losing order or fingerprints."
      },
      {
        q: "Can I rename a folder inside Envision?",
        a: "Yup. Hover the card and click the pencil button. This updates the display name in Envision without changing the folder on disk."
      },
    ],
  },
  {
    title: 'Files & safety',
    items: [
      {
        q: "Does Envision ever write inside my image folders?",
        a: "Only when you explicitly move an image between folders or create/rename a folder. Scanning, hashing, thumbnails, and ordering live in app data.",
      },
      {
        q: "Does reordering the grid move files?",
        a: "No. Order is stored in the index; your filenames and directories are untouched.",
      },
      {
        q: "Does duplicate detection delete or move anything?",
        a: "No. It only groups byte-identical files and badges them.",
      },
      {
        q: "Why is folder deletion missing?",
        a: "It is deliberately not implemented. Deleting user data irreversibly needs a recycle-bin story first.",
      },
      {
        q: "What if I move or rename a folder outside Envision?",
        a: "Envision notices via filesystem watching and prompts you to rescan; it will not silently rewrite things behind your back.",
      },
      {
        q: "A file changed on disk. Does Envision rescan automatically?",
        a: "It detects changes and offers a rescan; reconciliation is explicit so bursts of events do not race.",
      },
      {
        q: "What if a filename differs only by case (IMG vs img)?",
        a: "On macOS/Windows those are the same file; on Linux they are distinct. Envision follows your OS's filesystem rules.",
      },
    ],
  },
  {
    title: 'Formats & viewing',
    items: [
      {
        q: "Why are some images placeholders?",
        a: "HEIC, AVIF, JPEG XL, SVG, and camera raw are indexed and counted but not decoded by Envision; opening them uses your desktop's associated app.",
      },
      {
        q: "Why might a wide panorama look cropped in the grid?",
        a: "Very extreme aspect ratios are clamped so tiles stay readable; the full image in the viewer is unaffected.",
      },
      {
        q: "Why does the grid sometimes change shape on resize?",
        a: "Columns are recomputed from available width; your stored order is unchanged.",
      },
    ],
  },
  {
    title: 'Scanning & performance',
    items: [
      {
        q: "Why is the first scan slow?",
        a: "Phase 1 lists files quickly so the grid can render; phase 2 fingerprints (SHA-256) in the background. Large libraries take time, especially hashing.",
      },
      {
        q: "Can I keep using Envision while it scans?",
        a: "Yes - scanning and thumbnail generation run off the UI thread; the grid stays interactive.",
      },
      {
        q: "What decides when a file is re-read?",
        a: "Its size and modification time. Identity is always the full SHA-256 of bytes actually read, never the filename or dimensions.",
      },
      {
        q: "What does the thumbnail CPU limit do?",
        a: "Caps threads used for thumbnail generation across batches; scanning and hashing have their own pool. Applies to work started after the change.",
      },
      {
        q: "Why are thumbnails sometimes missing then appear?",
        a: "They are generated during the scan; a tile without one yet falls back to a placeholder instead of a broken image.",
      },
    ],
  },
  {
    title: 'Duplicates',
    items: [
      {
        q: "What counts as a duplicate?",
        a: "Files whose complete bytes hash to the same SHA-256 = identical content regardless of name or location.",
      },
      {
        q: "Why doesn't Envision just delete duplicates?",
        a: "Two copies in two folders are two copies you chose to keep; resolving them is your call in your file manager.",
      },
      {
        q: "Do duplicates share thumbnails?",
        a: "Yes. The thumbnail cache is keyed by content hash, so identical files reuse one decode.",
      },
    ],
  },
  {
    title: 'Moves & collisions',
    items: [
      {
        q: "What if I drop an image onto a folder that already has an image with the same file name?",
        a: "Envision renames it to “name (2).ext” and reports that it happened.",
      },
      {
        q: "What if a move crosses drives?",
        a: "It copies to a temp name in the destination, flushes, renames into place, then removes the source; so an interrupted move leaves the original intact.",
      },
      {
        q: "Does renaming a folder update its contents?",
        a: "Yes. The directory is renamed and every descendant path in the index is rewritten to match.",
      },
    ],
  },
  {
    title: 'Storage & privacy',
    items: [
      {
        q: "Where does Envision store its data?",
        a: "In your OS app-data directory: settings.db, plus libraries/<id>/index.db and thumbnails. Nothing goes into your image folders.",
      },
      {
        q: "Does Envision upload anything?",
        a: "No. It is local-first; gallery-dl is the only network path and only runs when you explicitly import.",
      },
      {
        q: "Are settings tied to a library?",
        a: "No. Theme, import defaults, and CPU limit are per-machine in settings.db.",
      },
      {
        q: "How do I back up or reset Envision's state?",
        a: "Back up or delete the app-data directory. Deleting index.db costs a rescan and nothing else.",
      },
      {
        q: "What does “Prune thumbnail cache” do?",
        a: "Removes thumbnails for superseded cache versions and files that are gone.",
      },
    ],
  },
  {
    title: 'Import (gallery-dl)',
    items: [
      {
        q: "gallery-dl isn't found.",
        a: "Install it separately and ensure it is on PATH; the Import tab shows the check.",
      },
      {
        q: "Browser cookies locked.",
        a: "Close the browser before importing from a private board.",
      },
      {
        q: "Where do imports go?",
        a: "Into a folder you choose (or a configured default); sections become subfolders. Afterwards you can add that folder as a library or rescan one that contains it.",
      },
    ],
  },
  {
    title: 'Troubleshooting',
    items: [
      {
        q: "What should I include in a bug report?",
        a: "Version, OS, and the app-data path; for image-loading issues, whether it is a release or a tauri dev build (dev uses a different thumbnail path).",
      },
      {
        q: "Why are some libraries reported unreadable in Stats?",
        a: "Their index.db could not be read; totals above are incomplete until they are relinked or re-added.",
      },
    ],
  },
];

function InfoTab() {
  const [diag, setDiag] = useState<{ text: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [diagError, setDiagError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [nameRes, versionRes, tauriRes, bundleRes, idRes] =
          await Promise.allSettled([
            getName(),
            getVersion(),
            getTauriVersion(),
            getBundleType(),
            getIdentifier(),
          ]);
        const name = nameRes.status === 'fulfilled' ? nameRes.value : '—';
        const version =
          versionRes.status === 'fulfilled' ? versionRes.value : (import.meta.env.PACKAGE_VERSION ?? '—');
        const tauriVersion =
          tauriRes.status === 'fulfilled' ? tauriRes.value : '—';
        const bundleType =
          bundleRes.status === 'fulfilled' ? String(bundleRes.value) : '—';
        const identifier =
          idRes.status === 'fulfilled' ? idRes.value : '—';

        const galleryDl = await checkGalleryDl().catch(() => false);

        let cacheLocation = '—';
        try {
          cacheLocation = (await libraryStats()).cache_location || '—';
        } catch {
          // Keep diagnostics useful even if stats cannot be read.
        }

        if (cancelled) return;

        const lines = [
          'Envision diagnostics',
          `App: ${name}`,
          `Version: ${version}`,
          `Build version (package.json): ${import.meta.env.PACKAGE_VERSION ?? '—'}`,
          `Tauri: ${tauriVersion}`,
          `Bundle: ${bundleType}`,
          `Identifier: ${identifier}`,
          `Platform: ${navigator.platform || '—'}`,
          `User agent: ${navigator.userAgent}`,
          `Screen: ${window.screen?.width ?? '?'}x${window.screen?.height ?? '?'} @${window.devicePixelRatio ?? 1}x`,
          `gallery-dl: ${galleryDl ? 'found' : 'missing'}`,
          `App data (libraries): ${cacheLocation}`,
        ];
        setDiag({ text: lines.join('\n') });
      } catch (error) {
        if (!cancelled) {
          setDiagError('Could not gather diagnostics.');
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const handleCopy = async () => {
    if (!diag) return;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(diag.text);
      } else {
        const textarea = document.createElement('textarea');
        textarea.value = diag.text;
        textarea.style.position = 'fixed';
        textarea.style.opacity = '0';
        document.body.appendChild(textarea);
        textarea.select();
        document.execCommand('copy');
        document.body.removeChild(textarea);
      }
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } catch {
      setDiagError('Could not copy to clipboard.');
    }
  };

  return (
    <div className="settings-pane">
      <section className="settings-group settings-about">
        <h3>About</h3>
        <div className="settings-group-heading">
          <span className="settings-field-label">Version</span>
          <span className="settings-field-label">Diagnostics</span>
        </div>
        <div className="settings-group-heading">
          <span className="settings-about-value">{import.meta.env.PACKAGE_VERSION ?? '—'}</span>
          <button
            type="button"
            onClick={() => void handleCopy()}
            disabled={!diag}
            title="Copy version, runtime, and environment details to the clipboard"
          >
            {copied ? 'Copied!' : diag ? 'Copy diagnostics' : 'Gathering…'}
          </button>
        </div>

        {diagError && (
          <p className="settings-error" role="alert">
            {diagError}
          </p>
        )}

        <h3>FAQ</h3>
        <div className="settings-faq">
          {FAQ.map(({ title, items }) => (
            <details key={title} className="settings-faq-category">
              <summary className="settings-faq-summary">{title}</summary>
              <dl className="settings-faq-list">
                {items.map(({ q, a }) => (
                  <div key={q} className="settings-faq-item">
                    <dt className="settings-faq-question">{q}</dt>
                    <dd className="settings-faq-answer">{a}</dd>
                  </div>
                ))}
              </dl>
            </details>
          ))}
        </div>

        <div className="settings-about-links">
          <a
            href="https://github.com/Yeosangist"
            target="_blank"
            rel="noopener noreferrer"
          >
            Developer: Yeosangist
          </a>
          <p>LICENSE: GPL V3</p>
          <a
            href="https://github.com/Yeosangist/Envision"
            target="_blank"
            rel="noopener noreferrer"
          >
            Source on GitHub
          </a>
        </div>
      </section>
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
