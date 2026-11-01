import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useSortable } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import { packMasonry, visibleIndices, type MasonryLayout } from './masonry/packer';
import type { Asset } from '../../types';
import { useSettingsStore } from '../../state/settingsStore';
import { generateThumbnailsForAssets } from '../../api/invoke';
import { useThumbnailReady } from '../../api/events';

export const TARGET_COLUMN_WIDTH = 210;
export const SPACING = 10;
/** Rows rendered above and below the viewport so scrolling does not flash. */
const OVERSCAN_PX = 600;

export interface MasonryGridProps {
  assets: Asset[];
  libraryId: string;
  thumbnailRevisions: Record<number, number>;
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
  libraryId: string;
  thumbnailRevision: number;
  selected: boolean;
  onOpen: (assetId: number) => void;
  onToggleSelect?: (assetId: number) => void;
}

function Tile({ asset, libraryId, thumbnailRevision, selected, onOpen, onToggleSelect }: TileProps) {
  const hideDuplicates = useSettingsStore((state) => state.hide_duplicates);
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: asset.id,
  });

  const [shown, setShown] = useState<{
    key: string;
    src: string;
    libraryId: string;
    relPath: string;
  } | null>(null);
  const [failedKey, setFailedKey] = useState<string | null>(null);
  const ratio = asset.width && asset.height ? `${asset.width} / ${asset.height}` : '1 / 1';
  const thumb = asset.thumb_url;
  const target = thumb
    ? {
        key: JSON.stringify([libraryId, asset.rel_path, thumb, thumbnailRevision]),
        src: thumb,
        libraryId,
        relPath: asset.rel_path,
      }
    : null;
  const currentShown = thumb && shown?.libraryId === libraryId && shown.relPath === asset.rel_path
    ? shown
    : null;
  const candidate =
    target && target.key !== currentShown?.key && target.key !== failedKey ? target : null;
  const imageLayers = [
    ...(currentShown ? [{ image: currentShown, isShown: true }] : []),
    ...(candidate ? [{ image: candidate, isShown: false }] : []),
  ];

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
        {!currentShown && (
          <div
            className="masonry-unsupported"
            title={
              target
                ? 'Thumbnail loading'
                : `${asset.ext.toUpperCase()} is not decoded by Envision and opens externally`
            }
          >
            <span className="masonry-unsupported-ext">
              {target ? (failedKey === target.key ? '·' : '') : asset.ext.toUpperCase()}
            </span>
          </div>
        )}
        {imageLayers.map(({ image, isShown }) => (
          <img
            key={image.key}
            src={image.src}
            alt={isShown ? asset.filename : ''}
            aria-hidden={!isShown}
            className={`masonry-image${isShown ? ' is-loaded' : ''}`}
            draggable={false}
            loading="lazy"
            decoding="async"
            onLoad={isShown ? undefined : () => setShown(image)}
            onError={isShown ? undefined : () => setFailedKey(image.key)}
          />
        ))}
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
  libraryId,
  thumbnailRevisions,
  scrollRef,
  onOpen,
  selectedIds = [],
  onToggleSelect,
}: MasonryGridProps) {
  const [width, setWidth] = useState(0);
  const [band, setBand] = useState({ top: 0, bottom: 0 });
  const [canvas, setCanvas] = useState<HTMLDivElement | null>(null);
  const requestedThumbnails = useRef(new Map<number, string>());

  useEffect(() => {
    requestedThumbnails.current.clear();
  }, [libraryId]);

  const onThumbnailReady = useCallback((event: { library_id: string; asset_id: number }) => {
    if (event.library_id === libraryId) requestedThumbnails.current.delete(event.asset_id);
  }, [libraryId]);
  useThumbnailReady(onThumbnailReady);

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

  const thumbnailRequests = useMemo(() => {
    const inViewport = visibleIndices(layout, band.top, band.bottom);
    const orderedIndices = [...inViewport, ...visible.filter((index) => !inViewport.includes(index))];
    return orderedIndices
      .map((index) => assets[index])
      .filter((asset) => asset?.decode_ok && asset.thumb_url)
      .map((asset) => ({ assetId: asset.id, cacheKey: asset.thumb_url! }));
  }, [assets, band.bottom, band.top, layout, visible]);

  useEffect(() => {
    const requests = thumbnailRequests.filter(({ assetId, cacheKey }) => {
      if (requestedThumbnails.current.get(assetId) === cacheKey) return false;
      requestedThumbnails.current.set(assetId, cacheKey);
      return true;
    });
    if (requests.length === 0) return;
    void generateThumbnailsForAssets(requests.map(({ assetId }) => assetId)).catch(() => {
      for (const { assetId } of requests) requestedThumbnails.current.delete(assetId);
    });
  }, [thumbnailRequests]);

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
              libraryId={libraryId}
              thumbnailRevision={thumbnailRevisions[asset.id] ?? 0}
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