import { useCallback, useEffect, useMemo, useState } from 'react';
import { useSortable } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { packMasonry, visibleIndices, type MasonryLayout } from './masonry/packer';
import type { Asset } from '../../types';
import { useSettingsStore } from '../../state/settingsStore';

export const TARGET_COLUMN_WIDTH = 210;
export const SPACING = 10;
/** Rows rendered above and below the viewport so scrolling does not flash. */
const OVERSCAN_PX = 600;

export interface MasonryGridProps {
  assets: Asset[];
  /** The scrollable ancestor. Shared with the folder row so one surface scrolls. */
  scrollRef: React.RefObject<HTMLElement | null>;
  onOpen: (assetId: number) => void;
  selectedIds?: number[];
  onToggleSelect?: (assetId: number) => void;
}

/** Drop target id for a folder card, distinguishable from a tile's numeric id. */
export const folderDropId = (folderId: number) => `folder:${folderId}`;

interface TileProps {
  asset: Asset;
  selected: boolean;
  onOpen: (assetId: number) => void;
  onToggleSelect?: (assetId: number) => void;
}

function Tile({ asset, selected, onOpen, onToggleSelect }: TileProps) {
  const hideDuplicates = useSettingsStore((state) => state.hide_duplicates);
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: asset.id,
  });

  const [thumbFailed, setThumbFailed] = useState(false);
  const ratio = asset.width && asset.height ? `${asset.width} / ${asset.height}` : '1 / 1';
  const thumb = asset.thumb_url;

  return (
    <div
      ref={setNodeRef}
      style={{ transform: CSS.Translate.toString(transform), transition }}
      className={`masonry-tile${isDragging ? ' is-dragging' : ''}${selected ? ' is-selected' : ''}`}
      {...attributes}
      {...listeners}
      onClick={() => {
        if (!isDragging) onOpen(asset.id);
      }}
    >
      <div className="masonry-frame" style={{ aspectRatio: ratio }}>
        {thumb && !thumbFailed ? (
          <img
            src={thumb}
            alt={asset.filename}
            className="masonry-image"
            draggable={false}
            loading="eager"
            decoding="async"
            onError={() => {
              setThumbFailed(true);
              import('@tauri-apps/api/core').then(({ invoke }) => {
                void invoke('generate_thumbnails_for_assets_cmd', { assetIds: [asset.id] });
              }).catch(() => {});
            }}
          />
        ) : (
          <div
            className="masonry-unsupported"
            title={
              thumbFailed
                ? 'Thumbnail unavailable'
                : `${asset.ext.toUpperCase()} is not decoded by Envision and opens externally`
            }
          >
            <span className="masonry-unsupported-ext">
              {thumbFailed ? '·' : asset.ext.toUpperCase()}
            </span>
          </div>
        )}
      </div>

      {!hideDuplicates && asset.duplicate_count > 0 && (
        <span
          className="masonry-duplicate-badge"
          title={`${asset.duplicate_count} other image${asset.duplicate_count === 1 ? '' : 's'} with identical contents`}
        >
          ×{asset.duplicate_count + 1}
        </span>
      )}

      {onToggleSelect && (
        <button
          type="button"
          className="masonry-select"
          aria-label={selected ? `Deselect ${asset.filename}` : `Select ${asset.filename}`}
          onPointerDown={(event) => event.stopPropagation()}
          onClick={(event) => {
            event.stopPropagation();
            onToggleSelect(asset.id);
          }}
        >
          {selected ? '✓' : '+'}
        </button>
      )}

      <span className="masonry-caption">{asset.filename}</span>
    </div>
  );
}

/** Virtualized masonry grid; drag-and-drop context is supplied by the parent. */
export function MasonryGrid({
  assets,
  scrollRef,
  onOpen,
  selectedIds = [],
  onToggleSelect,
}: MasonryGridProps) {
  const [width, setWidth] = useState(0);
  const [band, setBand] = useState({ top: 0, bottom: 0 });
  const [canvas, setCanvas] = useState<HTMLDivElement | null>(null);

  const measure = useCallback(() => {
    const element = scrollRef.current;
    if (!element) return;
    const rect = element.getBoundingClientRect();
    const top = Math.max(0, -rect.top + element.scrollTop);
    setBand({ top, bottom: top + rect.height });
  }, [scrollRef]);

  // Measure visibility from the scroller but pack to the canvas to exclude `.library-inner` padding.
  useEffect(() => {
    if (!canvas) return undefined;

    const updateWidth = () => {
      setWidth(canvas.getBoundingClientRect().width);
      measure();
    };

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      setWidth(entry.contentRect.width);
      measure();
    });
    observer.observe(canvas);

    // Recompute on window resize, including transitions out of maximized state.
    window.addEventListener('resize', updateWidth);
    updateWidth();

    return () => {
      observer.disconnect();
      window.removeEventListener('resize', updateWidth);
    };
  }, [canvas, measure]);

  // Render only the visible band plus overscan.
  useEffect(() => {
    const element = scrollRef.current;
    if (!element) return undefined;

    element.addEventListener('scroll', measure, { passive: true });
    measure();

    return () => element.removeEventListener('scroll', measure);
  }, [measure, scrollRef]);

  const layout: MasonryLayout = useMemo(
    () =>
      packMasonry(
        assets.map((asset) => ({
          id: asset.id,
          aspectRatio: asset.width && asset.height ? asset.height / asset.width : 1,
        })),
        { width, targetColumnWidth: TARGET_COLUMN_WIDTH, spacing: SPACING },
      ),
    [assets, width],
  );

  const visible = useMemo(
    () => visibleIndices(layout, band.top - OVERSCAN_PX, band.bottom + OVERSCAN_PX),
    [layout, band.top, band.bottom],
  );

  const visibleSet = useMemo(() => new Set(visible), [visible]);
  const selected = useMemo(() => new Set(selectedIds), [selectedIds]);

  if (assets.length === 0) return null;

  return (
    <div
      ref={setCanvas}
      className="masonry-canvas"
      style={{ height: layout.empty ? undefined : layout.totalHeight }}
    >
      {layout.tiles.map((tile) => {
        const asset = assets[tile.index];
        if (!visibleSet.has(tile.index)) return null;

        return (
          <div
            key={asset.id}
            className="masonry-positioned"
            style={{
              transform: `translate3d(${tile.x}px, ${tile.y}px, 0)`,
              width: tile.width,
              height: tile.height,
            }}
          >
            <Tile
              asset={asset}
              selected={selected.has(asset.id)}
              onOpen={onOpen}
              onToggleSelect={onToggleSelect}
            />
          </div>
        );
      })}
    </div>
  );
}