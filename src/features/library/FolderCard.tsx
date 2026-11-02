import { useState } from 'react';
import { useDroppable } from '@dnd-kit/core';
import { folderDropId } from './MasonryGrid';
import type { Folder } from '../../types';

export interface FolderCardProps {
  folder: Folder;
  onOpen: (folderId: number) => void;
  onRename?: (folder: Folder) => void;
}

/** Folder cards open directories; dropping an image here moves the file there. */
export function FolderCard({ folder, onOpen, onRename }: FolderCardProps) {
  const { setNodeRef, isOver } = useDroppable({ id: folderDropId(folder.id) });
  const [coverFailed, setCoverFailed] = useState(false);
  const cover = folder.cover_url && !coverFailed ? folder.cover_url : null;

  return (
    <div className={`folder-card${isOver ? ' is-over' : ''}`} ref={setNodeRef}>
      <button
        type="button"
        className="folder-card-open"
        onClick={() => onOpen(folder.id)}
        title={`${folder.name} — ${folder.image_count} image${folder.image_count === 1 ? '' : 's'}`}
      >
        <span className="folder-card-plate" aria-hidden="true">
          {cover && (
            <img
              src={cover}
              alt=""
              className="folder-card-cover"
              loading="lazy"
              decoding="async"
              draggable={false}
              onError={() => setCoverFailed(true)}
            />
          )}
        </span>

        <span className="folder-card-label">
          <span className="folder-card-name">{folder.name}</span>
          <span className="folder-card-count">{folder.image_count}</span>
        </span>
      </button>

      {onRename && (
        <button
          type="button"
          className="folder-card-rename"
          onClick={(event) => {
            event.stopPropagation();
            onRename(folder);
          }}
          aria-label={`Rename ${folder.name}`}
          title="Rename in Envision"
        >
          ✎
        </button>
      )}
    </div>
  );
}