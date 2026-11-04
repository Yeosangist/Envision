import { useCallback, useEffect, useRef, useState } from 'react';
import { listen } from '@tauri-apps/api/event';
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
import { addLibrary, cancelImportBoard, cancelScan, listLibraries, openAssetExternally } from './api/invoke';
import { CommandError, type ImportProgress, type Library, type ScanPhase } from './types';
import './index.css';

export default function App() {
  const [booted, setBooted] = useState(false);
  const [viewerId, setViewerId] = useState<number | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [importProgress, setImportProgress] = useState<ImportProgress | null>(null);
  const [importBoardPath, setImportBoardPath] = useState<string | null>(null);
  const [importLibraries, setImportLibraries] = useState<Library[]>([]);
  const [importActionBusy, setImportActionBusy] = useState(false);
  const [importError, setImportError] = useState<string | null>(null);
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

  const hideDuplicates = useSettingsStore((state) => state.hideDuplicates);
  const loadSettings = useSettingsStore((state) => state.load);

  const progress = useScanProgress();

  useEffect(() => {
    void loadSettings().then(() => setBooted(true));
    return watchSystemTheme();
  }, [loadSettings]);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void listen<ImportProgress>('import:progress', (event) => {
      if (!active) return;
      const next = event.payload;
      setImportProgress(next);
      if (next.phase === 'done') {
        void listLibraries().then((items) => {
          if (active) setImportLibraries(items);
        }).catch(() => {
          if (active) setImportLibraries([]);
        });
      }
    }).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    }).catch(() => {});

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const handleImportStarted = useCallback((boardPath: string) => {
    setImportBoardPath(boardPath);
    setImportError(null);
  }, []);

  const handleImportCancel = useCallback(async () => {
    try {
      await cancelImportBoard();
    } catch (failure) {
      setImportError(CommandError.from(failure).message);
    }
  }, []);

  const handleAddImportedLibrary = useCallback(async () => {
    if (!importBoardPath) return;
    setImportActionBusy(true);
    try {
      const added = await addLibrary(importBoardPath);
      setImportBoardPath(null);
      setImportProgress(null);
      await open(added.id);
    } catch (failure) {
      setImportError(CommandError.from(failure).message);
    } finally {
      setImportActionBusy(false);
    }
  }, [importBoardPath, open]);

  const handleRescanImportLibrary = useCallback(async (libraryId: string) => {
    setImportActionBusy(true);
    try {
      setImportBoardPath(null);
      setImportProgress(null);
      await open(libraryId);
    } finally {
      setImportActionBusy(false);
    }
  }, [open]);

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
        <LibraryLauncher
          onOpen={(id) => void open(id)}
          onOpenSettings={() => setSettingsOpen(true)}
          importActive={importProgress?.phase === 'downloading' || importBoardPath !== null}
          onImportStarted={handleImportStarted}
        />
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

      {importProgress && (
        <ImportTaskPanel
          progress={importProgress}
          error={importError}
          onCancel={() => void handleImportCancel()}
          onDismiss={() => {
            setImportProgress(null);
            setImportBoardPath(null);
            setImportError(null);
          }}
        />
      )}

      {importProgress?.phase === 'done' && importBoardPath && (
        <ImportFinishedDialog
          boardPath={importBoardPath}
          libraries={importLibraries.filter(
            (registered) => !registered.missing && isWithin(registered.root_path, importBoardPath),
          )}
          busy={importActionBusy}
          onLater={() => {
            setImportBoardPath(null);
            setImportProgress(null);
          }}
          onAdd={() => void handleAddImportedLibrary()}
          onRescan={(id) => void handleRescanImportLibrary(id)}
        />
      )}

      {importError && (
        <div className="toast toast-error" role="alert">
          <span>{importError}</span>
          <button type="button" onClick={() => setImportError(null)} aria-label="Dismiss">
            ✕
          </button>
        </div>
      )}

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

function isWithin(root: string, child: string): boolean {
  const normalizedRoot = root.replace(/\\/g, '/').replace(/\/+$/, '');
  const normalizedChild = child.replace(/\\/g, '/').replace(/\/+$/, '');
  return normalizedChild === normalizedRoot || normalizedChild.startsWith(`${normalizedRoot}/`);
}

function ImportTaskPanel({
  progress,
  error,
  onCancel,
  onDismiss,
}: {
  progress: ImportProgress;
  error: string | null;
  onCancel: () => void;
  onDismiss: () => void;
}) {
  const active = progress.phase === 'downloading';
  const failed = progress.phase === 'failed' || Boolean(error);
  const label = failed
    ? error || progress.error || 'Import failed'
    : progress.phase === 'done'
      ? 'Board download complete'
      : progress.phase === 'cancelled'
        ? 'Import cancelled; downloaded files were kept'
        : 'Downloading Pinterest board';

  return (
    <aside className={`import-task${failed ? ' is-failed' : ''}`} role="status" aria-live="polite">
      <div className="import-task-copy">
        <strong>{label}</strong>
        {active && <span>{progress.processed} pins downloaded</span>}
      </div>
      {active ? (
        <button type="button" onClick={onCancel}>Cancel</button>
      ) : (
        <button type="button" onClick={onDismiss} aria-label="Dismiss import status">×</button>
      )}
    </aside>
  );
}

function ImportFinishedDialog({
  boardPath,
  libraries,
  busy,
  onLater,
  onAdd,
  onRescan,
}: {
  boardPath: string;
  libraries: Library[];
  busy: boolean;
  onLater: () => void;
  onAdd: () => void;
  onRescan: (libraryId: string) => void;
}) {
  return (
    <>
      <button type="button" className="modal-scrim" aria-label="Finish later" onClick={onLater} />
      <section className="modal new-folder-modal" role="dialog" aria-modal="true" aria-labelledby="import-finished-title">
        <header className="modal-heading"><h2 id="import-finished-title">Board downloaded</h2></header>
        <div className="modal-body">
          <p>{boardPath}</p>
          <p>Add it as a library, or rescan an existing library that contains it.</p>
          {libraries.map((registered) => (
            <p key={registered.id}>
              <button type="button" onClick={() => onRescan(registered.id)} disabled={busy}>
                Rescan {registered.name}
              </button>
            </p>
          ))}
        </div>
        <footer className="new-folder-actions">
          <button type="button" onClick={onLater} disabled={busy}>Later</button>
          <button type="button" className="primary" onClick={onAdd} disabled={busy}>Add as library</button>
        </footer>
      </section>
    </>
  );
}