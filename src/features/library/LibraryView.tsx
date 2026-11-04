import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  DndContext,
  PointerSensor,
  TouchSensor,
  useSensor,
  useSensors,
  type DragEndEvent,
} from '@dnd-kit/core';
import { SortableContext } from '@dnd-kit/sortable';
import { MasonryGrid } from './MasonryGrid';
import { FolderCard } from './FolderCard';
import { Ancestry } from '../../components/Ancestry';
import { CloseIcon } from '../../components/icons';
import { useThumbnailReady } from '../../api/events';
import { ancestryOf, useLibraryStore } from '../../state/libraryStore';
import type { Folder } from '../../types';

export interface LibraryViewProps {
  onOpenAsset: (assetId: number) => void;
}

export function LibraryView({ onOpenAsset }: LibraryViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [newFolderOpen, setNewFolderOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<Folder | null>(null);
  const [thumbnailRevisions, setThumbnailRevisions] = useState<Record<number, number>>({});

  const {
    library,
    folders,
    childFolders,
    folderId,
    assets,
    loading,
    navigate,
    setOrder,
    moveToFolder,
    addFolder,
    renameFolderDisplay,
  } = useLibraryStore();

  const pendingThumbnails = assets.filter((a) => a.decode_ok && !a.thumb_url).length;

  const ancestry = useMemo(() => ancestryOf(folders, folderId), [folders, folderId]);

  const onThumbnailReady = useCallback((event: { library_id: string; asset_id: number }) => {
    const state = useLibraryStore.getState();
    if (state.library?.id !== event.library_id) return;
    if (!state.assets.some((asset) => asset.id === event.asset_id)) return;
    setThumbnailRevisions((current) => ({
      ...current,
      [event.asset_id]: (current[event.asset_id] ?? 0) + 1,
    }));
  }, []);
  useThumbnailReady(onThumbnailReady);

  const sensors = useSensors(
    // A short distance threshold keeps a click from starting a drag, so opening
    // the lightbox and rearranging the grid share one gesture.
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(TouchSensor, { activationConstraint: { delay: 220, tolerance: 8 } }),
  );

  const ids = useMemo(() => assets.map((asset) => asset.id), [assets]);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over) return;

      const overId = over.id;

      // Dropped on a folder card: this one really does move the file.
      if (typeof overId === 'string' && overId.startsWith('folder:')) {
        const target = Number(overId.slice('folder:'.length));
        const assetIds = selected.size > 0 ? [...selected] : [Number(active.id)];
        moveToFolder(assetIds, target);
        setSelected(new Set());
        return;
      }

      // Dropped on a tile: reorder, which writes positions to the index and
      // touches nothing on disk.
      if (typeof overId !== 'number' || active.id === overId) return;

      const from = ids.indexOf(Number(active.id));
      const to = ids.indexOf(overId);
      if (from === -1 || to === -1) return;

      const next = ids.slice();
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      setOrder(next);
    },
    [ids, moveToFolder, setOrder, selected],
  );

  const toggleSelect = useCallback((assetId: number) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(assetId)) next.delete(assetId);
      else next.add(assetId);
      return next;
    });
  }, []);

  return (
    <div className="library-view">
      <Ancestry ancestry={ancestry} onNavigate={navigate} libraryName={library?.name} />

      <div className="library-scroll" ref={scrollRef}>
        <div className="library-inner">
          <DndContext sensors={sensors} onDragEnd={handleDragEnd}>
            {childFolders && (
              <section className="folder-row" aria-label="Folders">
                {childFolders.map((folder) => (
                  <FolderCard key={folder.id} folder={folder} onOpen={navigate} onRename={setRenameTarget} />
                ))}
                <button
                  type="button"
                  className="folder-card folder-card-new"
                  onClick={() => setNewFolderOpen(true)}
                >
                  <span className="folder-card-plate folder-card-plate-new" aria-hidden="true">
                    ＋
                  </span>
                  <span className="folder-card-label">
                    <span className="folder-card-name">New folder</span>
                  </span>
                </button>
              </section>
            )}

            <div className="library-status">
              {loading
                ? 'Loading…'
                : `${assets.length} image${assets.length === 1 ? '' : 's'}${selected.size > 0 ? ` · ${selected.size} selected` : ''}${pendingThumbnails > 0 ? ` · ${pendingThumbnails} thumbnail${pendingThumbnails === 1 ? '' : 's'} generating` : ''} · drag to reorder · drop on a folder to move the file`}
              {selected.size > 0 && (
                <button
                  type="button"
                  className="library-status-action"
                  onClick={() => setSelected(new Set())}
                >
                  Deselect all
                </button>
              )}
            </div>

            {childFolders.length === 0 && assets.length === 0 && !loading && (
              <div className="library-empty">
                <p>No sub-folders yet.</p>
              </div>
            )}

            {assets.length === 0 && !loading && childFolders.length > 0 && (
              <div className="library-empty">
                <p>Nothing in this folder yet.</p>
                <p className="library-empty-hint">
                  Add images with your file manager and Envision will notice, or drop one
                  onto a folder above.
                </p>
              </div>
            )}

            <SortableContext items={ids}>
              <MasonryGrid
                assets={assets}
                libraryId={library?.id ?? ''}
                scrollRef={scrollRef}
                thumbnailRevisions={thumbnailRevisions}
                onOpen={onOpenAsset}
                selectedIds={[...selected]}
                onToggleSelect={toggleSelect}
              />
            </SortableContext>
          </DndContext>
        </div>
      </div>
      {newFolderOpen && (
        <NewFolderDialog
          onClose={() => setNewFolderOpen(false)}
          onCreate={async (name) => {
            await addFolder(name);
            setNewFolderOpen(false);
          }}
        />
      )}
      {renameTarget && (
        <RenameFolderDialog
          folder={renameTarget}
          onClose={() => setRenameTarget(null)}
          onRename={async (name) => {
            await renameFolderDisplay(renameTarget.id, name);
            setRenameTarget(null);
          }}
        />
      )}
    </div>
  );
}

interface NewFolderDialogProps {
  onClose: () => void;
  onCreate: (name: string) => Promise<void>;
}

function NewFolderDialog({ onClose, onCreate }: NewFolderDialogProps) {
  const [name, setName] = useState('');
  const [creating, setCreating] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !creating) onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [creating, onClose]);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed || creating) return;
    setCreating(true);
    try {
      await onCreate(trimmed);
    } finally {
      setCreating(false);
    }
  };

  return (
    <>
      <button
        type="button"
        className="modal-scrim"
        aria-label="Cancel new folder"
        onClick={() => !creating && onClose()}
      />
      <form className="modal new-folder-modal" role="dialog" aria-modal="true" aria-labelledby="new-folder-title" onSubmit={(event) => void handleSubmit(event)}>
        <header className="modal-heading">
          <h2 id="new-folder-title">New folder</h2>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            disabled={creating}
            aria-label="Cancel"
            title="Cancel"
          >
            <CloseIcon />
          </button>
        </header>
        <div className="modal-body new-folder-body">
          <label htmlFor="new-folder-name">Folder name</label>
          <input
            id="new-folder-name"
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            maxLength={255}
            placeholder="Enter a folder name"
            disabled={creating}
          />
        </div>
        <footer className="new-folder-actions">
          <button type="button" onClick={onClose} disabled={creating}>
            Cancel
          </button>
          <button type="submit" className="primary" disabled={!name.trim() || creating}>
            {creating ? 'Creating…' : 'Create folder'}
          </button>
        </footer>
      </form>
    </>
  );
}


interface RenameFolderDialogProps {
  folder: Folder;
  onClose: () => void;
  onRename: (name: string) => Promise<void>;
}

function RenameFolderDialog({ folder, onClose, onRename }: RenameFolderDialogProps) {
  const [name, setName] = useState(folder.name);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !saving) onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [saving, onClose]);

  const handleSubmit = async (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    const trimmed = name.trim();
    if (!trimmed || saving) return;
    setSaving(true);
    try {
      await onRename(trimmed);
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
        onClick={() => !saving && onClose()}
      />
      <form className="modal new-folder-modal" role="dialog" aria-modal="true" aria-labelledby="rename-folder-title" onSubmit={(event) => void handleSubmit(event)}>
        <header className="modal-heading">
          <h2 id="rename-folder-title">Rename folder</h2>
          <button
            type="button"
            className="icon-button"
            onClick={onClose}
            disabled={saving}
            aria-label="Cancel"
            title="Cancel"
          >
            <CloseIcon />
          </button>
        </header>
        <div className="modal-body new-folder-body">
          <label htmlFor="rename-folder-name">Folder name</label>
          <input
            id="rename-folder-name"
            autoFocus
            value={name}
            onChange={(event) => setName(event.target.value)}
            maxLength={255}
            placeholder="Enter a folder name"
            disabled={saving}
          />
          <p className="settings-hint">Renamed only inside Envision; files on disk are not affected.</p>
        </div>
        <footer className="new-folder-actions">
          <button type="button" onClick={onClose} disabled={saving}>
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
