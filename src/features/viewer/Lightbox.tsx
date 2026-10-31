import { useCallback, useEffect, useRef, useState } from 'react';
import { fetchFullImageUrl } from '../../api/assets';
import type { Asset } from '../../types';
import {
  IDENTITY,
  ZOOM_STEP,
  clampPan,
  isAtFit,
  panBy,
  zoomAt,
  type Matrix,
} from './transform';

export interface LightboxProps {
  assets: Asset[];
  currentId: number | null;
  onNavigate: (assetId: number) => void;
  onClose: () => void;
  onOpenExternally?: (assetId: number) => void;
}

/** Fetch full-resolution bytes by asset id and revoke the previous object URL. */
function useImageUrl(asset: Asset | null): { url: string | null; loading: boolean } {
  const [url, setUrl] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const requestId = useRef(0);

  useEffect(() => {
    // Bumping the id invalidates any in-flight request for the previous image,
    // so a fast arrow-key run cannot land the wrong picture on screen.
    const ticket = ++requestId.current;
    let revoked: string | null = null;

    if (!asset) {
      setUrl(null);
      return undefined;
    }

    setLoading(true);
    fetchFullImageUrl(asset)
      .then((next) => {
        if (requestId.current !== ticket) {
          if (next) URL.revokeObjectURL(next);
          return;
        }
        if (next) revoked = next;
        setUrl(next);
      })
      .catch(() => {
        if (requestId.current === ticket) setUrl(null);
      })
      .finally(() => {
        if (requestId.current === ticket) setLoading(false);
      });

    return () => {
      if (revoked) URL.revokeObjectURL(revoked);
    };
  }, [asset]);

  return { url, loading };
}

export function Lightbox({
  assets,
  currentId,
  onNavigate,
  onClose,
  onOpenExternally,
}: LightboxProps) {
  const [matrix, setMatrix] = useState<Matrix>(IDENTITY);
  const viewportRef = useRef<HTMLDivElement>(null);
  const dragOrigin = useRef<{ pointerX: number; pointerY: number; matrix: Matrix } | null>(null);
  const lastPressWasPrimary = useRef(true);

  const index = assets.findIndex((asset) => asset.id === currentId);
  const asset = index === -1 ? null : assets[index];
  const { url: imageUrl, loading } = useImageUrl(asset);

  const reset = useCallback(() => setMatrix(IDENTITY), []);

  // Reset on open and again on close, so the next visit starts at fit.
  useEffect(() => {
    reset();
  }, [currentId, reset]);

  useEffect(() => {
    if (currentId === null) return undefined;

    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        event.preventDefault();
        onClose();
      } else if (event.key === 'ArrowRight' && index < assets.length - 1) {
        event.preventDefault();
        onNavigate(assets[index + 1].id);
      } else if (event.key === 'ArrowLeft' && index > 0) {
        event.preventDefault();
        onNavigate(assets[index - 1].id);
      } else if ((event.key === '+' || event.key === '=') && viewportRef.current) {
        event.preventDefault();
        const rect = viewportRef.current.getBoundingClientRect();
        setMatrix((m) => zoomAt(m, rect.width / 2, rect.height / 2, ZOOM_STEP));
      } else if (event.key === '-' && viewportRef.current) {
        event.preventDefault();
        const rect = viewportRef.current.getBoundingClientRect();
        setMatrix((m) => zoomAt(m, rect.width / 2, rect.height / 2, 1 / ZOOM_STEP));
      } else if (event.key === '0') {
        event.preventDefault();
        reset();
      }
    };

    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [assets, currentId, index, onClose, onNavigate, reset]);

  const handleWheel = useCallback((event: React.WheelEvent) => {
    const viewport = viewportRef.current;
    if (!viewport) return;
    const rect = viewport.getBoundingClientRect();
    const x = event.clientX - rect.left;
    const y = event.clientY - rect.top;
    const factor = event.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP;
    setMatrix((m) => zoomAt(m, x, y, factor));
  }, []);

  const handlePointerDown = useCallback(
    (event: React.PointerEvent) => {
      lastPressWasPrimary.current = event.button === 0;
      if (event.button !== 0) return;
      const viewport = viewportRef.current;
      if (!viewport) return;
      const rect = viewport.getBoundingClientRect();
      dragOrigin.current = {
        pointerX: event.clientX - rect.left,
        pointerY: event.clientY - rect.top,
        matrix,
      };
      viewport.setPointerCapture(event.pointerId);
    },
    [matrix],
  );

  const handlePointerMove = useCallback((event: React.PointerEvent) => {
    const origin = dragOrigin.current;
    const viewport = viewportRef.current;
    if (!origin || !viewport) return;
    const rect = viewport.getBoundingClientRect();
    const dx = event.clientX - rect.left - origin.pointerX;
    const dy = event.clientY - rect.top - origin.pointerY;
    // Pan is a screen-space delta: the cursor should carry the image with it
    // one-for-one at any zoom, which is what `panBy` composes for.
    setMatrix(clampPan(panBy(origin.matrix, dx, dy)));
  }, []);

  const handlePointerUp = useCallback((event: React.PointerEvent) => {
    dragOrigin.current = null;
    viewportRef.current?.releasePointerCapture?.(event.pointerId);
  }, []);

  const handleDoubleClick = useCallback(() => {
    if (!lastPressWasPrimary.current) return;
    const viewport = viewportRef.current;
    if (!viewport) return;
    const rect = viewport.getBoundingClientRect();
    setMatrix((m) =>
      isAtFit(m) ? zoomAt(m, rect.width / 2, rect.height / 2, 2) : IDENTITY,
    );
  }, []);

  if (!asset) return null;

  return (
    <div className="lightbox" role="dialog" aria-modal="true" aria-label={asset.filename}>
      <button type="button" className="lightbox-scrim" aria-label="Close viewer" onClick={onClose} />

      <div className="lightbox-bar lightbox-bar-top">
        <div className="lightbox-heading">
          <span className="lightbox-title" title={asset.rel_path}>
            {asset.filename}
          </span>
          <span className="lightbox-position">
            {index + 1} / {assets.length}
          </span>
        </div>
        <button type="button" className="lightbox-close" onClick={onClose} aria-label="Close viewer">
          ✕
        </button>
      </div>

      <div
        ref={viewportRef}
        className="lightbox-viewport"
        onWheel={handleWheel}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
        onDoubleClick={handleDoubleClick}
      >
        <div
          className="lightbox-zoom-host"
          style={{ transform: `matrix(${matrix.join(',')})` }}
        >
          {asset.decode_ok ? (
            imageUrl ? (
              <img
                src={imageUrl}
                alt={asset.filename}
                className="lightbox-image"
                draggable={false}
              />
            ) : (
              <div className="lightbox-unsupported">
                <p>{loading ? 'Loading…' : 'This image could not be read.'}</p>
              </div>
            )
          ) : (
            <div className="lightbox-unsupported">
              <p>Envision does not decode {asset.ext.toUpperCase()}.</p>
              {onOpenExternally && (
                <button type="button" onClick={() => onOpenExternally(asset.id)}>
                  Open with the system viewer
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      <div className="lightbox-bar lightbox-bar-bottom">
        <button
          type="button"
          disabled={index <= 0}
          onClick={() => onNavigate(assets[index - 1].id)}
          aria-label="Previous image"
        >
          ‹
        </button>
        <span className="lightbox-hint">
          scroll to zoom · drag to pan · double-click to reset
        </span>
        <button
          type="button"
          disabled={index >= assets.length - 1}
          onClick={() => onNavigate(assets[index + 1].id)}
          aria-label="Next image"
        >
          ›
        </button>
        {!asset.decode_ok && onOpenExternally && (
          <button type="button" onClick={() => onOpenExternally(asset.id)}>
            Open externally
          </button>
        )}
      </div>
    </div>
  );
}