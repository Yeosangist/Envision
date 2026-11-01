import type { FolderId } from '../state/libraryStore';
import type { Folder } from '../types';

export interface AncestryProps {
  ancestry: Folder[];
  onNavigate: (folderId: FolderId) => void;
  /** Trailing text, e.g. a live result count. */
  suffix?: string;
  /** Library name to display for the root folder instead of "Library". */
  libraryName?: string;
}

/** Breadcrumb trail that preserves the current folder when ancestors overflow. */
export function Ancestry({ ancestry, onNavigate, suffix, libraryName }: AncestryProps) {
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
                  {folder.rel_path === '' ? (libraryName || 'Library') : folder.name}
                </span>
              ) : (
                <button
                  type="button"
                  className="ancestry-link"
                  onClick={() => onNavigate(folder.id)}
                  title={folder.rel_path || folder.name}
                >
                  {folder.rel_path === '' ? (libraryName || 'Library') : folder.name}
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