import { useCallback, useEffect, useRef, useState } from 'react';
import { LibraryLauncher } from './features/launcher/LibraryLauncher';
import { SettingsModal } from './features/settings/SettingsModal';
import { LibraryView } from './features/library/LibraryView';
import { DuplicatesView } from './features/library/DuplicatesView';
import { Lightbox } from './features/viewer/Lightbox';
import { ScanIndicator } from './components/ScanIndicator';
import { HomeIcon, RefreshIcon, CogIcon } from './components/icons';
import { useLibraryStore } from './state/libraryStore';
import { useSettingsStore, watchSystemTheme } from './state/settingsStore';
import { useScanProgress, useLibraryChanged } from './api/events';
import { cancelScan, openAssetExternally } from './api/invoke';
import type { ScanPhase } from './types';
import './index.css';

export default function App() {
  const [booted, setBooted] = useState(false);
  const [viewerId, setViewerId] = useState<number | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const readyMs = useRef(performance.now()).current;

  const library = useLibraryStore((state) => state.library);
  const assets = useLibraryStore((state) => state.assets);
  const view = useLibraryStore((state) => state.view);
  const history = useLibraryStore((state) => state.history);
  const future = useLibraryStore((state) => state.future);
  const scanning = useLibraryStore((state) => state.scanning);
  const redundant = useLibraryStore((state) => state.redundant);
  const notice = useLibraryStore((state) => state.notice);
  const error = useLibraryStore((state) => state.error);
  const {
    open,
    close,
    back,
    forward,
    setView,
    rescan,
    dismissNotice,
    dismissError,
  } = useLibraryStore();

  const hideDuplicates = useSettingsStore((state) => state.hide_duplicates);
  const loadSettings = useSettingsStore((state) => state.load);

  const progress = useScanProgress();

  useEffect(() => {
    void loadSettings().then(() => setBooted(true));
    return watchSystemTheme();
  }, [loadSettings]);

  // Fingerprinting continues after the directory walk; clear busy state and refresh duplicate badges on the terminal event.
  const lastPhase = useRef<ScanPhase | null>(null);
  useEffect(() => {
    // Ignore anything still in flight from the library we just closed.
    if (!progress || !library || progress.library_id !== library.id) return;

    const phase = progress.phase;
    if (phase === lastPhase.current) return;
    lastPhase.current = phase;

    if (phase !== 'done' && phase !== 'cancelled' && phase !== 'failed') return;

    void useLibraryStore.getState().rescanFinished();
  }, [progress, library]);

  // Rescan after external filesystem changes so the grid stays current.
  const handleFilesystemChange = useCallback(() => {
    void useLibraryStore.getState().rescan().then(() => {
      void useLibraryStore.getState().refresh();
    });
  }, []);

  useLibraryChanged(handleFilesystemChange);

  // Recheck on focus in case the platform missed a filesystem event.
  useEffect(() => {
    if (!library) return undefined;
    const onFocus = () => {
      void useLibraryStore.getState().refresh();
    };
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [library]);

  const handleOpenAsset = useCallback((assetId: number) => setViewerId(assetId), []);

  const handleOpenExternally = useCallback(async (assetId: number) => {
    try {
      await openAssetExternally(assetId);
    } catch {
    /* the desktop reports its own failure */
    }
  }, []);

  useEffect(() => {
    if (hideDuplicates && view !== 'browse') {
      void setView('browse');
    }
  }, [hideDuplicates, view, setView]);

  if (!booted) {
    return <div className="boot" />;
  }

  return (
    <div className="shell">
      {!library ? (
        <LibraryLauncher onOpen={(id) => void open(id)} onOpenSettings={() => setSettingsOpen(true)} />
      ) : (
        <>
          <header className="shell-bar">
            <div className="shell-bar-nav">
              <button
                type="button"
                className="icon-button"
                onClick={() => void close()}
                aria-label="Libraries"
                title="Libraries"
              >
                <HomeIcon />
              </button>
              <button
                type="button"
                onClick={() => void back()}
                disabled={history.length === 0}
                aria-label="Back"
              >
                ‹
              </button>
              <button
                type="button"
                onClick={() => void forward()}
                disabled={future.length === 0}
                aria-label="Forward"
              >
                ›
              </button>
            </div>

            <div className="shell-bar-title" title={library.root_path}>
              {library.name}
            </div>

            <div className="shell-bar-actions">
              {!hideDuplicates && (
                <button
                  type="button"
                  className={view === 'browse' ? 'is-active' : ''}
                  onClick={() => void setView('browse')}
                >
                  Browse
                </button>
              )}
              {!hideDuplicates && (
                <button
                  type="button"
                  className={view === 'duplicates' ? 'is-active' : ''}
                  onClick={() => void setView('duplicates')}
                  title="Images with identical contents — detection only, nothing is changed on disk"
                >
                  Duplicates
                  {redundant > 0 && <span className="shell-bar-badge">{redundant}</span>}
                </button>
              )}
              <button
                type="button"
                className="icon-button"
                onClick={() => void rescan()}
                disabled={scanning}
                aria-label="Rescan"
                title="Rescan this library"
              >
                <RefreshIcon />
              </button>
              {scanning && (
                <button type="button" onClick={() => void cancelScan()}>
                  Stop
                </button>
              )}
              <button
                type="button"
                className="icon-button"
                onClick={() => setSettingsOpen(true)}
                aria-label="Settings"
                title="Settings"
              >
                <CogIcon />
              </button>
            </div>
          </header>

          <main className="shell-body">
            {view === 'browse' ? (
              <LibraryView onOpenAsset={handleOpenAsset} />
            ) : (
              <DuplicatesView onOpenAsset={handleOpenAsset} />
            )}
          </main>
        </>
      )}

      {library && <ScanIndicator progress={progress} />}

      {notice && (
        <div className="toast" role="status">
          <span>{notice}</span>
          <button type="button" onClick={dismissNotice} aria-label="Dismiss">
            ✕
          </button>
        </div>
      )}

      {error && (
        <div className="toast toast-error" role="alert">
          <span>{error}</span>
          <button type="button" onClick={dismissError} aria-label="Dismiss">
            ✕
          </button>
        </div>
      )}

      {settingsOpen && (
        <SettingsModal onClose={() => setSettingsOpen(false)} readyMs={readyMs} />
      )}

      <Lightbox
        assets={assets}
        currentId={viewerId}
        onNavigate={setViewerId}
        onClose={() => setViewerId(null)}
        onOpenExternally={handleOpenExternally}
      />
    </div>
  );
}