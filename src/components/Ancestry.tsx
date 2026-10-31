import type { FolderId } from '../state/libraryStore';
import type { Folder } from '../types';

export interface AncestryProps {
  ancestry: Folder[];
  onNavigate: (folderId: FolderId) => void;
  suffix?: string;
}

export function Ancestry({ ancestry, onNavigate, suffix }: AncestryProps) {
  if (ancestry.length === 0) return null;

  return (
    <nav className="ancestry" aria-label="Breadcrumb">
      <ol className="ancestry-list">
        {ancestry.map((folder, index) => {
          const isLast = index === ancestry.length - 1;
          return (
            <li key={folder.id} className="ancestry-item">
              {index > 0 && (
                <span className="ancestry-separator" aria-hidden="true">
                  /
                </span>
              )}
              {isLast ? (
                <span className="ancestry-current" title={folder.rel_path || folder.name}>
                  {folder.rel_path === '' ? 'Library' : folder.name}
                </span>
              ) : (
                <button
                  type="button"
                  className="ancestry-link"
                  onClick={() => onNavigate(folder.id)}
                  title={folder.rel_path || folder.name}
                >
                  {folder.rel_path === '' ? 'Library' : folder.name}
                </button>
              )}
            </li>
          );
        })}
      </ol>
      {suffix && <span className="ancestry-suffix">{suffix}</span>}
    </nav>
  );
}