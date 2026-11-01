
/** Thumbnails use scoped asset URLs; originals use IPC bytes because WebKitGTK blocks custom schemes. */
import { invoke } from '@tauri-apps/api/core';
import type { Asset } from '../types';

/** Minimal MIME map for building the lightbox Blob. */
export function mimeFor(ext: string): string {
  const table: Record<string, string> = {
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    jfif: 'image/jpeg',
    png: 'image/png',
    gif: 'image/gif',
    webp: 'image/webp',
    bmp: 'image/bmp',
    tif: 'image/tiff',
    tiff: 'image/tiff',
  };
  return table[ext.toLowerCase()] ?? 'application/octet-stream';
}

/** Fetch original bytes as an object URL, or return `null` for externally opened formats. */
export async function fetchFullImageUrl(asset: Asset): Promise<string | null> {
  if (!asset.decode_ok) return null;

  // Use raw bytes to avoid base64-encoding full-resolution images.
  const bytes = await invoke<ArrayBuffer | number[]>('read_asset_bytes', {
    assetId: asset.id,
  });

  const blob = new Blob([new Uint8Array(bytes as ArrayBuffer)], {
    type: mimeFor(asset.ext),
  });
  return URL.createObjectURL(blob);
}
