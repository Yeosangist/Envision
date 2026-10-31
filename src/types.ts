
/** Shapes returned by the Rust backend. Mirrors `src-tauri/src/domain/mod.rs`. */
export interface Library {
  id: string;
  name: string;
  root_path: string;
  created_at: number;
  last_opened_at: number | null;
  /** True when the registered folder no longer exists on disk. */
  missing: boolean;
}

export interface Folder {
  id: number;
  parent_id: number | null;
  name: string;
  /** '' for the library root, otherwise 'parent/child', always '/'-separated. */
  rel_path: string;
  /** Number of images directly inside this folder. */
  image_count: number;
  /** Asset-protocol URL for the thumbnail of this folder's first image. */
  cover_url: string | null;
}

export interface Asset {
  id: number;
  folder_id: number;
  filename: string;
  rel_path: string;
  ext: string;
  width: number | null;
  height: number | null;
  decode_ok: boolean;
  position: number;
  /** Other images in the library with byte-identical contents. */
  duplicate_count: number;
  /** Asset-protocol URL for the cached thumbnail, or null when unavailable. */
  thumb_url: string | null;
}

export interface DuplicateGroup {
  sha256: string;
  bytes: number;
  count: number;
  assets: Asset[];
}

export interface DuplicateSummary {
  groups: number;
  redundant: number;
}

export interface MoveResult {
  asset_id: number;
  from_folder_id: number;
  to_folder_id: number;
  filename: string;
  rel_path: string;
  renamed: boolean;
}

export interface AssetPosition {
  asset_id: number;
  position: number;
}

export interface ReorderResult {
  folder_id: number;
  positions: AssetPosition[];
}

export interface ScanReport {
  library_id: string;
  added: number;
  updated: number;
  removed: number;
  duplicates: number;
  fingerprinted: number;
  folders: number;
}

export type ScanPhase = 'scanning' | 'fingerprinting' | 'done' | 'cancelled' | 'failed';

export interface ScanProgress {
  library_id: string;
  phase: ScanPhase;
  processed: number;
  total: number;
  added: number;
  updated: number;
  removed: number;
  duplicates: number;
  current: string | null;
}

export interface Settings {
  theme: string;
  interface_scale: number;
  gif_autoplay: boolean;
  max_playing_gifs: number;
  folder_card_size: number;
  hide_duplicates: boolean;
  custom_text: string;
  custom_text_muted: string;
  custom_accent: string;
  custom_background: string;
  custom_background_image: string | null;
  custom_background_blur: number;
  import_folder: string | null;
  import_ask: boolean;
  thumbnail_cpu_limit: number;
}

/** Per-library counts shown on the home screen. */
export interface LibrarySummary {
  id: string;
  images: number;
  /** Sub-folders, excluding the library root. */
  folders: number;
}

/** Totals across every library Envision tracks, plus the derived cache. */
export interface Stats {
  libraries: number;
  /** Libraries whose index could not be read, so the totals are incomplete. */
  unreadable: number;
  images: number;
  folders: number;
  duplicate_groups: number;
  redundant: number;
  /** Size of the indexed images, as recorded when they were indexed. */
  library_bytes: number;
  thumbnails_bytes: number;
  index_bytes: number;
  /** Application data — where Envision's own cache lives, never a user folder. */
  cache_location: string;
}

/** A backend failure with a stable category, safe to show a user. */
export class CommandError extends Error {
  readonly category: string;

  constructor(category: string, message: string) {
    super(message);
    this.name = 'CommandError';
    this.category = category;
  }

  static from(error: unknown): CommandError {
    if (error && typeof error === 'object' && 'category' in error && 'message' in error) {
      const shaped = error as { category: string; message: string };
      return new CommandError(shaped.category, shaped.message);
    }
    return new CommandError('unknown', error instanceof Error ? error.message : String(error));
  }
}