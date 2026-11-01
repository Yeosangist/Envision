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
import { ancestryOf, useLibraryStore } from '../../state/libraryStore';

export interface LibraryViewProps {
  onOpenAsset: (assetId: number) => void;
}

export function LibraryView({ onOpenAsset }: LibraryViewProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [newFolderOpen, setNewFolderOpen] = useState(false);

  const {
    folders,
    childFolders,
    folderId,
    assets,
    loading,
    navigate,
    setOrder,
    moveToFolder,
    addFolder,
  } = useLibraryStore();

  const ancestry = useMemo(() => ancestryOf(folders, folderId), [folders, folderId]);

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
      <Ancestry ancestry={ancestry} onNavigate={navigate} />

      <div className="library-scroll" ref={scrollRef}>
        <div className="library-inner">
          <DndContext sensors={sensors} onDragEnd={handleDragEnd}>
            {childFolders && (
              <section className="folder-row" aria-label="Folders">
                {childFolders.map((folder) => (
                  <FolderCard key={folder.id} folder={folder} onOpen={navigate} />
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
                : `${assets.length} image${assets.length === 1 ? '' : 's'} · drag to reorder · drop on a folder to move the file`}
            </div>

            {childFolders.length === 0 && assets.length === 0 && !loading && (
              <div className="library-empty">
                <p>No sub-folders yet.</p>
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
                scrollRef={scrollRef}
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