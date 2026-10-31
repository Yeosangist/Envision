import { useMemo } from 'react';
import { useLibraryStore } from '../../state/libraryStore';

/** SHA-256 duplicates are surfaced without modifying or hiding any files. */
export function DuplicatesView({ onOpenAsset }: { onOpenAsset: (assetId: number) => void }) {
  const { duplicates, loading, folderId } = useLibraryStore();

  const totalRedundant = useMemo(
    () => duplicates.reduce((sum, group) => sum + (group.count - 1), 0),
    [duplicates],
  );

  if (loading) return <div className="library-empty">Loading duplicates…</div>;

  if (duplicates.length === 0) {
    return (
      <div className="library-empty">
        <p>No duplicates found.</p>
        <p className="library-empty-hint">
          Envision compares the SHA-256 of complete file contents, so two files match
          only when every byte matches — never because the names or sizes look alike.
          {folderId === null && ' Scan a library to populate this view.'}
        </p>
      </div>
    );
  }

  return (
    <div className="duplicates-view">
      <p className="duplicates-summary">
        {duplicates.length} group{duplicates.length === 1 ? '' : 's'} · {totalRedundant}{' '}
        redundant cop{totalRedundant === 1 ? 'y' : 'ies'}. Nothing has been changed on
        disk.
      </p>

      {duplicates.map((group) => (
        <section key={group.sha256} className="duplicate-group">
          <header className="duplicate-header">
            <span className="duplicate-hash" title={group.sha256}>
              {group.sha256.slice(0, 12)}…
            </span>
            <span className="duplicate-count">{group.count} copies</span>
            <span className="duplicate-bytes">{formatBytes(group.bytes)} each</span>
          </header>

          <div className="duplicate-grid">
            {group.assets.map((asset) => (
              <button
                key={asset.id}
                type="button"
                className="duplicate-tile"
                onClick={() => onOpenAsset(asset.id)}
                title={`${asset.rel_path} — click to view`}
              >
                {asset.thumb_url ? (
                  <img src={asset.thumb_url} alt={asset.filename} loading="lazy" />
                ) : (
                  <span className="duplicate-unsupported">{asset.ext.toUpperCase()}</span>
                )}
                <span className="duplicate-path">{asset.rel_path}</span>
              </button>
            ))}
          </div>
        </section>
      ))}
    </div>
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`;
}