import { create } from 'zustand';
import {
  createFolder,
  duplicateSummary,
  listAssets,
  listChildFolders,
  listFolders,
  listDuplicates,
  moveAssets,
  openLibrary,
  reorderAssets,
  scanLibrary,
} from '../api/invoke';
import { CommandError, type Asset, type DuplicateGroup, type Folder, type Library } from '../types';

export type FolderId = number | null;

export type View = 'browse' | 'duplicates';

interface LibraryState {
  library: Library | null;
  folders: Folder[];
  childFolders: Folder[];
  folderId: FolderId;
  assets: Asset[];
  duplicates: DuplicateGroup[];
  redundant: number;
  view: View;

  history: FolderId[];
  future: FolderId[];

  loading: boolean;
  scanning: boolean;
  lastOpenMs: number | null;
  error: string | null;
  notice: string | null;

  open: (libraryId: string) => Promise<void>;
  close: () => Promise<void>;
  navigate: (folderId: FolderId) => Promise<void>;
  back: () => Promise<void>;
  forward: () => Promise<void>;
  setView: (view: View) => Promise<void>;
  refreshDuplicates: () => Promise<void>;
  rescanFinished: () => Promise<void>;

  refresh: () => Promise<void>;
  rescan: () => Promise<void>;
  setOrder: (orderedIds: number[]) => Promise<void>;
  moveToFolder: (assetIds: number[], folderId: number) => Promise<void>;
  addFolder: (name: string) => Promise<void>;
  dismissNotice: () => void;
  dismissError: () => void;
}

function message(error: unknown): string {
  return error instanceof CommandError ? error.message : String(error);
}

export const useLibraryStore = create<LibraryState>((set, get) => ({
  library: null,
  folders: [],
  childFolders: [],
  folderId: null,
  assets: [],
  duplicates: [],
  redundant: 0,
  view: 'browse',
  history: [],
  future: [],
  loading: false,
  scanning: false,
  lastOpenMs: null,
  error: null,
  notice: null,

  open: async (libraryId) => {
    const started = performance.now();
    set({ loading: true, error: null });
    try {
      const library = await openLibrary(libraryId);
      set({ library, folderId: null, history: [], future: [], view: 'browse' });

      await get().rescan();
      await get().refresh();
      set({ lastOpenMs: performance.now() - started });
    } catch (error) {
      set({ error: message(error) });
    } finally {
      set({ loading: false });
    }
  },

  close: async () => {
    set({
      library: null,
      folders: [],
      childFolders: [],
      assets: [],
      folderId: null,
      history: [],
      future: [],
      scanning: false,
      loading: false,
    });
  },

  navigate: async (folderId) => {
    const { folderId: current, history } = get();
    if (folderId === current) return;
    set({
      history: [...history, current],
      future: [],
      folderId,
      view: 'browse',
    });
    await get().refresh();
  },

  back: async () => {
    const { history, folderId, future } = get();
    const previous = history[history.length - 1];
    if (previous === undefined) return;
    set({
      history: history.slice(0, -1),
      future: [folderId, ...future],
      folderId: previous,
      view: 'browse',
    });
    await get().refresh();
  },

  forward: async () => {
    const { future, folderId, history } = get();
    const next = future[0];
    if (next === undefined) return;
    set({
      future: future.slice(1),
      history: [...history, folderId],
      folderId: next,
      view: 'browse',
    });
    await get().refresh();
  },

  setView: async (view) => {
    set({ view });
    if (view === 'duplicates') {
      await get().refreshDuplicates();
    }
  },

  refreshDuplicates: async () => {
    set({ loading: true });
    try {
      const [duplicates, summary] = await Promise.all([listDuplicates(), duplicateSummary()]);
      set({ duplicates, redundant: summary.redundant });
    } catch (error) {
      set({ error: message(error) });
    } finally {
      set({ loading: false });
    }
  },

  rescanFinished: async () => {
    set({ scanning: false });
    await get().refresh();
    await get().refreshDuplicates();
  },

  refresh: async () => {
    const { folderId, view } = get();
    if (view === 'duplicates') return;

    set({ loading: true });
    try {
      const folders = await listFolders();
      set({ folders });

      const resolved = resolveFolderId(folderId, folders);
      const [assets, childFolders] = await Promise.all([
        listAssets(resolved),
        listChildFolders(folderId),
      ]);

      set({ assets, childFolders });

      try {
        const { invoke } = await import('@tauri-apps/api/core');
        void invoke('generate_thumbnails_for_folder_cmd', { folderId: resolved });
      } catch {
      }
    } catch (error) {
      set({ error: message(error) });
    } finally {
      set({ loading: false });
    }
  },

  rescan: async () => {
    set({ scanning: true });
    try {
      await scanLibrary();
    } catch (error) {
      set({ scanning: false, error: message(error) });
    }
  },

  setOrder: async (orderedIds) => {
    const previous = get().assets;
    const byId = new Map(previous.map((asset) => [asset.id, asset]));
    const reordered = orderedIds
      .map((id) => byId.get(id))
      .filter((asset): asset is Asset => Boolean(asset));

    if (reordered.length !== previous.length) return;
    set({ assets: reordered.map((asset, index) => ({ ...asset, position: index })) });

    try {
      const resolved = resolveFolderId(get().folderId, get().folders);
      await reorderAssets(resolved, orderedIds);
    } catch (error) {
      set({ assets: previous, error: message(error) });
    }
  },

  moveToFolder: async (assetIds, folderId) => {
    try {
      const results = await moveAssets(assetIds, folderId);
      const renamed = results.filter((result) => result.renamed).length;
      set({
        notice: results.length === 1
          ? `Moved ${results[0].filename}${renamed ? ' (renamed to avoid a clash)' : ''}.`
          : `Moved ${results.length} images${renamed ? `, ${renamed} renamed to avoid a clash` : ''}.`,
      });
      await get().refresh();
    } catch (error) {
      set({ error: message(error) });
    }
  },

  addFolder: async (name) => {
    try {
      const parent = resolveFolderId(get().folderId, get().folders);
      const folder = await createFolder(parent, name);
      set({ folders: await listFolders(), notice: `Created “${folder.name}”.` });
    } catch (error) {
      set({ error: message(error) });
    }
  },

  dismissNotice: () => set({ notice: null }),
  dismissError: () => set({ error: null }),
}));

export function resolveFolderId(folderId: FolderId, folders: Folder[]): number {
  if (folderId !== null) return folderId;

  const root = folders.find((folder) => folder.rel_path === '');
  if (!root) {
    throw new CommandError('not_found', 'This library has not been indexed yet.');
  }
  return root.id;
}

export function ancestryOf(folders: Folder[], folderId: FolderId): Folder[] {
  const byId = new Map(folders.map((folder) => [folder.id, folder]));
  const chain: Folder[] = [];
  let cursor = folderId === null ? folders.find((f) => f.rel_path === '') : byId.get(folderId);
  let guard = 0;

  while (cursor && guard < 512) {
    chain.unshift(cursor);
    cursor = cursor.parent_id === null ? undefined : byId.get(cursor.parent_id);
    guard += 1;
  }
  return chain;
}

export function folderFor(folders: Folder[], rel: string): Folder | undefined {
  return folders.find((folder) => folder.rel_path === rel);
}