
import { invoke } from '@tauri-apps/api/core';
import type { Asset } from '../types';

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

export async function fetchFullImageUrl(asset: Asset): Promise<string | null> {
  if (!asset.decode_ok) return null;

  const bytes = await invoke<ArrayBuffer | number[]>('read_asset_bytes', {
    assetId: asset.id,
  });

  const blob = new Blob([new Uint8Array(bytes as ArrayBuffer)], {
    type: mimeFor(asset.ext),
  });
  return URL.createObjectURL(blob);
}