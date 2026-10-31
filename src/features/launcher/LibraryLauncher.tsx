import { useCallback, useEffect, useState } from 'react';
import { open, confirm } from '@tauri-apps/plugin-dialog';
import {
  addLibrary,
  forgetLibrary,
  librarySummaries,
  listLibraries,
  relinkLibrary,
} from '../../api/invoke';
import { CommandError, type Library, type LibrarySummary } from '../../types';
import { CogIcon } from '../../components/icons';

export interface LibraryLauncherProps {
  onOpen: (libraryId: string) => void;
  onOpenSettings: () => void;
}

/** "123 images · 3 sections", from the per-library counts. */
function describeCounts(summary: LibrarySummary | undefined): string {
  const images = summary?.images ?? 0;
  const folders = summary?.folders ?? 0;
  return `${images.toLocaleString()} ${images === 1 ? 'image' : 'images'} · ${folders.toLocaleString()} ${
    folders === 1 ? 'section' : 'sections'
  }`;
}

export function LibraryLauncher({ onOpen, onOpenSettings }: LibraryLauncherProps) {
  const [libraries, setLibraries] = useState<Library[]>([]);
  const [summaries, setSummaries] = useState<Record<string, LibrarySummary>>({});
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

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

  const handleForget = async (library: Library) => {
    const confirmed = await confirm(
      `Forget “${library.name}”?\n\nThis removes Envision's index and cached thumbnails for it. Your folder and images are left exactly as they are.`,
      {
        title: 'Forget library',
        kind: 'warning',
      },
    );
    if (!confirmed) return;

    try {
      setBusy(library.id);
      await forgetLibrary(library.id);
      await refresh();
    } catch (failure) {
      setError(CommandError.from(failure).message);
    } finally {
      setBusy(null);
    }
  };

  return (
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
        <button type="button" onClick={() => onOpenSettings()} disabled={busy !== null}>
          Import board
        </button>
        <button type="button" onClick={refresh} disabled={loading}>
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

      <div className="launcher-cards">
        {libraries.map((library) => {
          const summary = summaries[library.id];
          return (
            <article
              key={library.id}
              className={`launcher-card${library.missing ? ' is-missing' : ''}`}
            >
              <header className="launcher-card-header">
                <span className="launcher-card-name">{library.name}</span>
                <span className="launcher-card-counts">{describeCounts(summary)}</span>
              </header>
              <span className="launcher-card-path" title={library.root_path}>
                {library.root_path}
              </span>
              {library.missing && <span className="launcher-badge">Folder moved or removed</span>}
              <div className="launcher-card-actions">
                {library.missing ? (
                  <>
                    <button type="button" onClick={() => handleRelink(library)} disabled={busy === library.id}>
                      Relink…
                    </button>
                    <button type="button" className="danger" onClick={() => handleForget(library)} disabled={busy === library.id}>
                      Forget
                    </button>
                  </>
                ) : (
                  <>
                    <button type="button" className="primary" onClick={() => onOpen(library.id)} disabled={busy === library.id}>
                      Open
                    </button>
                    <button type="button" className="danger" onClick={() => handleForget(library)} disabled={busy === library.id}>
                      Forget
                    </button>
                  </>
                )}
              </div>
            </article>
          );
        })}
      </div>
    </div>
  );
}