
export interface Library {
  id: string;
  name: string;
  root_path: string;
  created_at: number;
  last_opened_at: number | null;
  missing: boolean;
}

export interface Folder {
  id: number;
  parent_id: number | null;
  name: string;
  rel_path: string;
  image_count: number;
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
  duplicate_count: number;
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

export interface LibrarySummary {
  id: string;
  images: number;
  folders: number;
}

export interface Stats {
  libraries: number;
  unreadable: number;
  images: number;
  folders: number;
  duplicate_groups: number;
  redundant: number;
  library_bytes: number;
  thumbnails_bytes: number;
  index_bytes: number;
  cache_location: string;
}

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