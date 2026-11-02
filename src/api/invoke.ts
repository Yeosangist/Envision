
/** Typed wrappers for the Rust command surface. */
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
  SettingsDto,
  Stats,
} from '../types';

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw CommandError.from(error);
  }
}


// --- libraries -------------------------------------------------------------
export const addLibrary = (path: string, name?: string) =>
  call<Library>('add_library', { request: { path, name: name ?? null } });

export const listLibraries = () => call<Library[]>('list_libraries');

export const openLibrary = (libraryId: string) =>
  call<Library>('open_library', { libraryId });

export const closeLibrary = () => call<void>('close_library');

/** Forget a library: removes Envision's registration and derived data only. */
export const forgetLibrary = (libraryId: string) =>
  call<void>('forget_library', { libraryId });

export const relinkLibrary = (libraryId: string, path: string) =>
  call<Library>('relink_library', { libraryId, path });

export const renameLibrary = (libraryId: string, name: string) =>
  call<Library>('rename_library', { libraryId, name });


// --- scanning --------------------------------------------------------------
export const scanLibrary = () => call<ScanReport>('scan_library');

export const cancelScan = () => call<void>('cancel_scan');

export const cancelImportBoard = () => call<void>('cancel_import_board');


// --- folders and assets ----------------------------------------------------
export const listChildFolders = (parentId: number | null) =>
  call<Folder[]>('list_child_folders', { parentId });

export const listFolders = () => call<Folder[]>('list_folders');

export const folderAncestry = (folderId: number) =>
  call<Folder[]>('folder_ancestry', { folderId });

export const listAssets = (folderId: number) => call<Asset[]>('list_assets', { folderId });

/** Generate thumbnails for an ordered batch of visible/near-visible assets. */
export const generateThumbnailsForAssets = (assetIds: number[]) =>
  call<void>('generate_thumbnails_for_assets_cmd', { assetIds });

/** Persist display order in the index without touching files. */
export const reorderAssets = (folderId: number, orderedIds: number[]) =>
  call<ReorderResult>('reorder_assets', { folderId, orderedIds });

/** Move images between real folders. This one does touch the filesystem. */
export const moveAssets = (assetIds: number[], toFolderId: number) =>
  call<MoveResult[]>('move_assets', { assetIds, toFolderId });

export const createFolder = (parentId: number, name: string) =>
  call<Folder>('create_folder', { parentId, name });

export const renameFolder = (folderId: number, name: string) =>
  call<Folder>('rename_folder', { folderId, name });

export const renameFolderDisplay = (folderId: number, name: string) =>
  call<Folder>('rename_folder_display', { folderId, name });

/** Read original image bytes by ID without exposing a filesystem path. */
export const readAssetBytes = (assetId: number) =>
  invoke<ArrayBuffer>('read_asset_bytes', { assetId });

export const openAssetExternally = (assetId: number) =>
  call<void>('open_asset_externally', { assetId });


// --- duplicates ------------------------------------------------------------
export const listDuplicates = () => call<DuplicateGroup[]>('list_duplicates');

export const duplicateSummary = () => call<DuplicateSummary>('duplicate_summary');


// --- settings --------------------------------------------------------------
function settingsFromDto(settings: SettingsDto): Settings {
  return {
    theme: settings.theme,
    interfaceScale: settings.interface_scale,
    gifAutoplay: settings.gif_autoplay,
    maxPlayingGifs: settings.max_playing_gifs,
    folderCardSize: settings.folder_card_size,
    hideDuplicates: settings.hide_duplicates,
    importFolder: settings.import_folder,
    importAsk: settings.import_ask,
    thumbnailCpuLimit: settings.thumbnail_cpu_limit,
    importBrowserProfile: settings.import_browser_profile,
    customText: settings.custom_text,
    customTextMuted: settings.custom_text_muted,
    customAccent: settings.custom_accent,
    customBackground: settings.custom_background,
    customBackgroundImage: settings.custom_background_image,
    customBackgroundBlur: settings.custom_background_blur,
  };
}

function settingsToDto(settings: Settings): SettingsDto {
  return {
    theme: settings.theme,
    interface_scale: settings.interfaceScale,
    gif_autoplay: settings.gifAutoplay,
    max_playing_gifs: settings.maxPlayingGifs,
    folder_card_size: settings.folderCardSize,
    hide_duplicates: settings.hideDuplicates,
    import_folder: settings.importFolder,
    import_ask: settings.importAsk,
    thumbnail_cpu_limit: settings.thumbnailCpuLimit,
    import_browser_profile: settings.importBrowserProfile,
    custom_text: settings.customText,
    custom_text_muted: settings.customTextMuted,
    custom_accent: settings.customAccent,
    custom_background: settings.customBackground,
    custom_background_image: settings.customBackgroundImage,
    custom_background_blur: settings.customBackgroundBlur,
  };
}

export const getSettings = async () => settingsFromDto(await call<SettingsDto>('get_settings'));
export const checkGalleryDl = () => call<boolean>('check_gallery_dl');

export const updateSettings = async (settings: Settings) =>
  settingsFromDto(await call<SettingsDto>('update_settings', { settings: settingsToDto(settings) }));

/** Set or clear the background image and return its basename. */
export const setThemeBackground = (path: string | null) =>
  call<string | null>('set_theme_background', { path });

/** The stored background image's bytes, for wrapping in a Blob URL. */
export const readThemeBackground = () => invoke<ArrayBuffer>('read_theme_background');


// --- statistics ------------------------------------------------------------
export const libraryStats = () => call<Stats>('library_stats');
export const pruneAllThumbnails = () => call<number>('prune_all_thumbnails');

export const librarySummaries = () => call<LibrarySummary[]>('library_summaries');