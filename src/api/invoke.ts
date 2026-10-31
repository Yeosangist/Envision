
import { invoke } from '@tauri-apps/api/core';
import {
  Asset,
  CommandError,
  DuplicateGroup,
  DuplicateSummary,
  Folder,
  Library,
  LibrarySummary,
  MoveResult,
  ReorderResult,
  ScanReport,
  Settings,
  Stats,
} from '../types';

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw CommandError.from(error);
  }
}


export const addLibrary = (path: string, name?: string) =>
  call<Library>('add_library', { request: { path, name: name ?? null } });

export const listLibraries = () => call<Library[]>('list_libraries');

export const openLibrary = (libraryId: string) =>
  call<Library>('open_library', { libraryId });

export const closeLibrary = () => call<void>('close_library');

export const forgetLibrary = (libraryId: string) =>
  call<void>('forget_library', { libraryId });

export const relinkLibrary = (libraryId: string, path: string) =>
  call<Library>('relink_library', { libraryId, path });


export const scanLibrary = () => call<ScanReport>('scan_library');

export const cancelScan = () => call<void>('cancel_scan');


export const listChildFolders = (parentId: number | null) =>
  call<Folder[]>('list_child_folders', { parentId });

export const listFolders = () => call<Folder[]>('list_folders');

export const folderAncestry = (folderId: number) =>
  call<Folder[]>('folder_ancestry', { folderId });

export const listAssets = (folderId: number) => call<Asset[]>('list_assets', { folderId });

export const reorderAssets = (folderId: number, orderedIds: number[]) =>
  call<ReorderResult>('reorder_assets', { folderId, orderedIds });

export const moveAssets = (assetIds: number[], toFolderId: number) =>
  call<MoveResult[]>('move_assets', { assetIds, toFolderId });

export const createFolder = (parentId: number, name: string) =>
  call<Folder>('create_folder', { parentId, name });

export const renameFolder = (folderId: number, name: string) =>
  call<Folder>('rename_folder', { folderId, name });

export const readAssetBytes = (assetId: number) =>
  invoke<ArrayBuffer>('read_asset_bytes', { assetId });

export const openAssetExternally = (assetId: number) =>
  call<void>('open_asset_externally', { assetId });


export const listDuplicates = () => call<DuplicateGroup[]>('list_duplicates');

export const duplicateSummary = () => call<DuplicateSummary>('duplicate_summary');


export const getSettings = () => call<Settings>('get_settings');
export const checkGalleryDl = () => call<boolean>('check_gallery_dl');

export const updateSettings = (settings: Settings) =>
  call<Settings>('update_settings', { settings });

export const setThemeBackground = (path: string | null) =>
  call<string | null>('set_theme_background', { path });

export const readThemeBackground = () => invoke<ArrayBuffer>('read_theme_background');


export const libraryStats = () => call<Stats>('library_stats');
export const pruneAllThumbnails = () => call<number>('prune_all_thumbnails');

export const librarySummaries = () => call<LibrarySummary[]>('library_summaries');