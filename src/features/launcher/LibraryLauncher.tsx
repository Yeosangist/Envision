import { useCallback, useEffect, useMemo, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import {
  addLibrary,
  forgetLibrary,
  librarySummaries,
  listLibraries,
  relinkLibrary,
  renameLibrary,
} from '../../api/invoke';
import { CommandError, type Library, type LibrarySummary } from '../../types';
import { CloseIcon, CogIcon } from '../../components/icons';
import { ImportBoardModal } from '../import/ImportBoardModal';

export interface LibraryLauncherProps {
  onOpen: (libraryId: string) => void;
  onOpenSettings: () => void;
  importActive: boolean;
  onImportStarted: (boardPath: string) => void;
}

/** "123 images · 3 sections", from the per-library counts. */
function describeCounts(summary: LibrarySummary | undefined): string {
  const images = summary?.images ?? 0;
  const folders = summary?.folders ?? 0;
  return `${images.toLocaleString()} ${images === 1 ? 'image' : 'images'} · ${folders.toLocaleString()} ${
    folders === 1 ? 'section' : 'sections'
  }`;
}

function displayLibraryPath(path: string): string {
  const displayedPath = path.replace(/^\/home\/[^/]+\/?/, '');
  return displayedPath || '~';
}

/** Home-screen list of registered image folders. */
export function LibraryLauncher({ onOpen, onOpenSettings, importActive, onImportStarted }: LibraryLauncherProps) {
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [summaries, setSummaries] = useState<Record<string, LibrarySummary>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [forgetTarget, setForgetTarget] = useState<Library | null>(null);
  const [renameTarget, setRenameTarget] = useState<Library | null>(null);
  const [showImportModal, setShowImportModal] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      setLibraries(await listLibraries());
      setError(null);
    } catch (failure) {
      setError(CommandError.from(failure).message);
    } finally {
      setLoading(false);
    }

    // Load counts separately so unavailable summaries never block the library list.
    try {
      const counts = await librarySummaries();
      setSummaries(Object.fromEntries(counts.map((summary) => [summary.id, summary])));
    } catch {
      setSummaries({});
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const alphabetizedLibraries = useMemo(
    () =>
      [...libraries].sort((a, b) =>
        a.name.localeCompare(b.name, undefined, { sensitivity: 'base', numeric: true }),
      ),
    [libraries],
  );

  const handleAdd = async () => {
    try {
      const picked = await open({ directory: true, multiple: false, title: 'Choose a folder of images' });
      if (typeof picked !== 'string') return;

      setBusy('adding');
      const library = await addLibrary(picked);
      await refresh();
      onOpen(library.id);
    } catch (failure) {
      setError(CommandError.from(failure).message);
    } finally {
      setBusy(null);
    }
  };

  const handleRelink = async (library: Library) => {
    try {
      const picked = await open({ directory: true, multiple: false, title: `Locate “${library.name}”` });
      if (typeof picked !== 'string') return;

      setBusy(library.id);
      const updated = await relinkLibrary(library.id, picked);
      await refresh();
      onOpen(updated.id);
    } catch (failure) {
      setError(CommandError.from(failure).message);
    } finally {
      setBusy(null);
    }
  };

  const handleForget = async () => {
    if (!forgetTarget) return;
    const library = forgetTarget;
    try {
      setBusy(library.id);
      await forgetLibrary(library.id);
      setForgetTarget(null);
      await refresh();
    } catch (failure) {
      setError(CommandError.from(failure).message);
    } finally {
      setBusy(null);
    }
  };

  return (
    <div className="launcher-screen">
      <div className="launcher">
        <header className="launcher-header">
          <div className="launcher-header-row">
            <div>
              <h1>Envision</h1>
              <p>A local-first viewer for the images you already have.</p>
            </div>
            <button
              type="button"
              className="icon-button"
              onClick={onOpenSettings}
              aria-label="Settings"
              title="Settings"
            >
              <CogIcon />
            </button>
          </div>
        </header>

        <div className="launcher-actions">
          <button type="button" className="primary" onClick={handleAdd} disabled={busy !== null}>
            Add a library
          </button>
          <button type="button" onClick={() => setShowImportModal(true)} disabled={busy !== null || importActive}>
            Import board
          </button>
          <button type="button" onClick={() => void refresh()} disabled={loading || busy !== null}>
            Refresh
          </button>
        </div>

        {error && (
          <div className="launcher-error" role="alert">
            {error}
          </div>
        )}

        {loading && libraries.length === 0 && <p className="launcher-hint">Loading libraries…</p>}

        {!loading && libraries.length === 0 && (
          <p className="launcher-hint">
            No libraries yet. Choose a folder of images to get started — nothing is copied,
            and Envision never writes inside it unless you move a file yourself.
          </p>
        )}
      </div>

      <section className="launcher-cards" data-count={alphabetizedLibraries.length} aria-label="Libraries">
        {alphabetizedLibraries.map((library) => {
          const summary = summaries[library.id];
          const displayPath = displayLibraryPath(library.root_path);
          return (
            <article
              key={library.id}
              className={`launcher-card${library.missing ? ' is-missing' : ''}`}
            >
              <header className="launcher-card-header">
                <span className="launcher-card-name">{library.name}</span>
                <span className="launcher-card-counts">{describeCounts(summary)}</span>
                <button
                  type="button"
                  className="launcher-card-rename"
                  onClick={(event) => {
                    event.stopPropagation();
                    setRenameTarget(library);
                  }}
                  aria-label={`Rename ${library.name}`}
                  title="Rename in Envision"
                >
                  ✎
                </button>
              </header>
              <span className="launcher-card-path" title={displayPath}>
                {displayPath}
              </span>
              {library.missing && <span className="launcher-badge">Folder moved or removed</span>}
              <div className="launcher-card-actions">
                {library.missing ? (
                  <>
                    <button type="button" onClick={() => handleRelink(library)} disabled={busy === library.id}>
                      Relink…
                    </button>
                    <button type="button" className="danger" onClick={() => setForgetTarget(library)} disabled={busy === library.id}>
                      Forget
                    </button>
                  </>
                ) : (
                  <>
                    <button type="button" className="primary" onClick={() => onOpen(library.id)} disabled={busy === library.id}>
                      Open
                    </button>
                    <button type="button" className="danger" onClick={() => setForgetTarget(library)} disabled={busy === library.id}>
                      Forget
                    </button>
                  </>
                )}
              </div>
            </article>
          );
        })}
      </section>
      {forgetTarget && (
        <ForgetLibraryDialog
          library={forgetTarget}
          busy={busy === forgetTarget.id}
          onCancel={() => setForgetTarget(null)}
          onConfirm={() => void handleForget()}
        />
      )}
      {renameTarget && (
        <RenameLibraryDialog
          library={renameTarget}
          onCancel={() => setRenameTarget(null)}
          onConfirm={async (name) => {
            setBusy(renameTarget.id);
            try {
              await renameLibrary(renameTarget.id, name);
              await refresh();
              setRenameTarget(null);
            } catch (failure) {
              setError(CommandError.from(failure).message);
            } finally {
              setBusy(null);
            }
          }}
        />
      )}
      <ImportBoardModal
        isOpen={showImportModal}
        onClose={() => setShowImportModal(false)}
        onStarted={onImportStarted}
      />
    </div>
  );
}

interface ForgetLibraryDialogProps {
  library: Library;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

function ForgetLibraryDialog({ library, busy, onCancel, onConfirm }: ForgetLibraryDialogProps) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !busy) onCancel();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [busy, onCancel]);

  return (
    <>
      <button
        type="button"
        className="modal-scrim"
        aria-label="Cancel forgetting library"
        onClick={() => !busy && onCancel()}
      />
      <section
        className="modal new-folder-modal forget-library-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="forget-library-title"
      >
        <header className="modal-heading">
          <h2 id="forget-library-title">Forget library</h2>
          <button
            type="button"
            className="icon-button"
            onClick={onCancel}
            disabled={busy}
            aria-label="Cancel"
            title="Cancel"
          >
            <CloseIcon />
          </button>
        </header>
        <div className="modal-body forget-library-body">
          <p>Forget "{library.name}"?</p>
          <p>
            This removes Envision&apos;s index and cached thumbnails for it. Your folder and images
            are left exactly as they are.
          </p>
        </div>
        <footer className="new-folder-actions">
          <button type="button" onClick={onCancel} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="danger" onClick={onConfirm} disabled={busy}>
            Forget
          </button>
        </footer>
      </section>
    </>
  );
}

interface RenameLibraryDialogProps {
  library: Library;
  onCancel: () => void;
  onConfirm: (name: string) => Promise<void>;
}

function RenameLibraryDialog({ library, onCancel, onConfirm }: RenameLibraryDialogProps) {
  const [name, setName] = useState(library.name);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !saving) onCancel();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [saving, onCancel]);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed || saving) return;
    setSaving(true);
    try {
      await onConfirm(trimmed);
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <button
        type="button"
        className="modal-scrim"
        aria-label="Cancel rename"
        onClick={() => !saving && onCancel()}
      />
      <form className="modal new-folder-modal" role="dialog" aria-modal="true" aria-labelledby="rename-library-title" onSubmit={(event) => void handleSubmit(event)}>
        <header className="modal-heading">
          <h2 id="rename-library-title">Rename library</h2>
          <button
            type="button"
            className="icon-button"
            onClick={onCancel}
            disabled={saving}
            aria-label="Cancel"
            title="Cancel"
          >
            <CloseIcon />
          </button>
        </header>
        <div className="modal-body new-folder-body">
          <label htmlFor="rename-library-name">Library name</label>
          <input
            id="rename-library-name"
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            maxLength={255}
            placeholder="Enter a library name"
            disabled={saving}
          />
          <p className="settings-hint">Renamed only inside Envision; the folder on disk is not moved.</p>
        </div>
        <footer className="new-folder-actions">
          <button type="button" onClick={onCancel} disabled={saving}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={!name.trim() || saving}>
            {saving ? 'Saving…' : 'Rename'}
          </button>
        </footer>
      </form>
    </>
  );
}