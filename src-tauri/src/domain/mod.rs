
//! Shared IPC types. Images are addressed by asset id, never by filesystem path.
use serde::{Deserialize, Serialize};
use std::path::Path;

/// A registered image library. `root_path` is resolved by the backend on every
/// use; the frontend only ever echoes back the opaque `id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: i64,
    pub last_opened_at: Option<i64>,
    /// True when `root_path` no longer resolves to a directory on disk. The
    /// library is retained so the user can relink or forget it.
    pub missing: bool,
}

/// A directory within a library. `rel_path` is `''` for the root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    /// Number of images directly inside this folder.
    pub image_count: i64,
    /// URL for the first decodable image's cached thumbnail, or `None` if unavailable.
    pub cover_url: Option<String>,
}

/// One indexed image, described relative to its library root.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: i64,
    pub folder_id: i64,
    pub filename: String,
    pub rel_path: String,
    pub ext: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub bytes: i64,
    pub mtime: i64,
    pub sha256: Option<String>,
    pub decode_ok: bool,
    pub position: i64,
    /// Number of *other* indexed assets in the library with identical bytes.
    pub duplicate_count: i64,
    /// Backend-built thumbnail URL, never a filesystem path, or `None` if undecodable.
    pub thumb_url: Option<String>,
}

/// A set of assets sharing one SHA-256. Detection only: nothing in this type
/// ever causes a file to be moved, renamed, or deleted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub sha256: String,
    pub bytes: i64,
    pub count: i64,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScanPhase {
    /// Walking the directory tree and reconciling the index.
    Scanning,
    /// Reading file bytes to compute SHA-256 fingerprints.
    Fingerprinting,
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub library_id: String,
    pub phase: ScanPhase,
    /// Files discovered during the walk (phase 1) or fingerprinted (phase 2).
    /// Number of pins downloaded or skipped.
    pub processed: u64,
    pub total: u64,
    /// Files added, updated, or removed from the index during the walk.
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub duplicates: u64,
    /// Current pin being downloaded, if available.
    pub current: Option<String>,
}

/// Result of a reorder request. Carries the authoritative positions so the UI
/// can reconcile without re-reading the whole folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderResult {
    pub folder_id: i64,
    /// `asset_id -> position` for every asset in the folder.
    pub positions: Vec<AssetPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPosition {
    pub asset_id: i64,
    pub position: i64,
}

/// Outcome of moving one or more files between real directories.
/// Result of validating an import request before starting gallery-dl.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveResult {
    pub asset_id: i64,
    pub from_folder_id: i64,
    pub to_folder_id: i64,
    pub filename: String,
    pub rel_path: String,
    /// True when the destination already held a file with that name and the
    /// file had to be renamed.
    pub renamed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub theme: String,
    pub interface_scale: f64,
    pub gif_autoplay: bool,
    pub max_playing_gifs: i64,
    pub folder_card_size: i64,
    /// Hide duplicate-detection entry points only; detection and indexed groups remain unchanged.
    pub hide_duplicates: bool,
    /// Where gallery-dl should download a board by default. `None` means ask.
    pub import_folder: Option<String>,
    /// Prompt for a destination even when `import_folder` is set.
    pub import_ask: bool,
    /// Maximum number of CPU cores to use for thumbnail generation. `0` means use all available cores.
    pub thumbnail_cpu_limit: i64,
    /// Custom theme colours, `#rrggbb`, read only when `theme` is `custom`.
    pub custom_text: String,
    pub custom_text_muted: String,
    pub custom_accent: String,
    pub custom_background: String,
    /// Background image basename; Rust reads its stored path and sends bytes to the webview.
    pub custom_background_image: Option<String>,
    /// Blur radius, in CSS pixels, applied to the background image alone.
    pub custom_background_blur: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            interface_scale: 1.0,
            gif_autoplay: true,
            max_playing_gifs: 4,
            folder_card_size: 160,
            hide_duplicates: false,
            import_folder: None,
            import_ask: true,
            thumbnail_cpu_limit: 0,
            // Seeded from the grey palette: the most common starting point for
            // a custom theme, and a legible one before the user picks anything.
            custom_text: "#ececed".to_string(),
            custom_text_muted: "#9a9aa2".to_string(),
            custom_accent: "#60a5fa".to_string(),
            custom_background: "#101012".to_string(),
            custom_background_image: None,
            custom_background_blur: 12.0,
        }
    }
}

/// Normalize a six-digit hex color, returning `fallback` for invalid input.
pub fn normalize_hex(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    let digits = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("#{}", digits.to_ascii_lowercase())
    } else {
        fallback.to_string()
    }
}

impl Settings {
    /// Clamp stored settings to usable values on both reads and writes.
    pub fn normalized(mut self) -> Self {
        self.theme = match self.theme.trim().to_ascii_lowercase().as_str() {
            "light" => "light".to_string(),
            "dark" | "grey" => "grey".to_string(),
            "oled" => "oled".to_string(),
            "custom" => "custom".to_string(),
            _ => "system".to_string(),
        };
        if !self.interface_scale.is_finite() {
            self.interface_scale = 1.0;
        }
        self.interface_scale = self.interface_scale.clamp(0.75, 1.5);
        self.max_playing_gifs = self.max_playing_gifs.clamp(1, 12);
        self.folder_card_size = self.folder_card_size.clamp(96, 320);

        // Clamp CPU limit to a reasonable range. 0 means "use all cores",
        // and negative values are treated as 0.
        if self.thumbnail_cpu_limit < 0 {
            self.thumbnail_cpu_limit = 0;
        }
        // Cap at 32 to prevent absurd values, but allow systems with many cores.
        self.thumbnail_cpu_limit = self.thumbnail_cpu_limit.min(32);

        let defaults = Settings::default();
        self.custom_text = normalize_hex(&self.custom_text, &defaults.custom_text);
        self.custom_text_muted =
            normalize_hex(&self.custom_text_muted, &defaults.custom_text_muted);
        self.custom_accent = normalize_hex(&self.custom_accent, &defaults.custom_accent);
        self.custom_background =
            normalize_hex(&self.custom_background, &defaults.custom_background);

        if !self.custom_background_blur.is_finite() {
            self.custom_background_blur = defaults.custom_background_blur;
        }
        self.custom_background_blur = self.custom_background_blur.clamp(0.0, 40.0);

        // Keep only the base name. The field crosses into the webview, and a
        // stray directory in a hand-edited database must not ride along with it.
        self.custom_background_image = self
            .custom_background_image
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .and_then(|value| {
                std::path::Path::new(&value)
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
            })
            .filter(|value| !value.is_empty());

        self
    }
}

/// Totals for one library's index, counted without writing to it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryTotals {
    pub images: i64,
    pub folders: i64,
    pub duplicate_groups: i64,
    pub redundant: i64,
    pub bytes: i64,
}

/// Per-library home-screen counts; unindexed libraries report zeroes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub id: String,
    pub images: i64,
    /// Sub-folders, excluding the library root.
    pub folders: i64,
}

/// Settings-screen totals across registered libraries and app-data storage.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub libraries: i64,
    /// Number of registered libraries whose indexes could not be read.
    pub unreadable: i64,
    pub images: i64,
    pub folders: i64,
    pub duplicate_groups: i64,
    pub redundant: i64,
    /// Sum of image sizes recorded in indexes, not a fresh measurement of user files.
    pub library_bytes: i64,
    pub thumbnails_bytes: i64,
    pub index_bytes: i64,
    /// Application-data location of the derived cache.
    pub cache_location: String,
}

/// Derive a stable ID from an absolute root so re-adding it reuses its index.
pub fn library_id_for(root: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

/// Convert a native relative path into the '/'-separated form stored in SQLite.
pub fn to_rel_string(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            std::path::Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn settings_clamp_out_of_range_values() {
        let settings = Settings {
            theme: "NEON".into(),
            interface_scale: 9.0,
            gif_autoplay: true,
            max_playing_gifs: 99,
            folder_card_size: 4,
            ..Settings::default()
        }
        .normalized();

        assert_eq!(settings.theme, "system");
        assert_eq!(settings.interface_scale, 1.5);
        assert_eq!(settings.max_playing_gifs, 12);
        assert_eq!(settings.folder_card_size, 96);
    }

    #[test]
    fn settings_normalize_nan_scale() {
        let settings = Settings {
            interface_scale: f64::NAN,
            ..Settings::default()
        }
        .normalized();

        assert_eq!(settings.interface_scale, 1.0);
    }

    #[test]
    fn thumbnail_cpu_limit_is_clamped_and_zero_means_all_cores() {
        let cases = [(-8, 0), (0, 0), (2, 2), (32, 32), (99, 32)];

        for (stored, expected) in cases {
            let settings = Settings {
                thumbnail_cpu_limit: stored,
                ..Settings::default()
            }
            .normalized();

            assert_eq!(
                settings.thumbnail_cpu_limit, expected,
                "stored {stored} should normalise to {expected}"
            );
            assert_eq!(
                settings.clone().normalized().thumbnail_cpu_limit,
                expected,
                "normalisation must be idempotent"
            );
        }
    }

    /// Regression: legacy `dark` maps to `grey`, and supported theme names survive normalization.
    #[test]
    fn theme_names_are_migrated_and_idempotent() {
        let renamed = Settings {
            theme: "DARK".into(),
            ..Settings::default()
        }
        .normalized();
        assert_eq!(renamed.theme, "grey");
        assert_eq!(renamed.clone().normalized(), renamed);

        for name in ["light", "grey", "oled", "custom", "system"] {
            let settings = Settings {
                theme: name.into(),
                ..Settings::default()
            }
            .normalized();
            assert_eq!(settings.theme, name);
        }
    }

    #[test]
    fn custom_colours_fall_back_when_they_are_not_colours() {
        let settings = Settings {
            custom_text: "rebeccapurple".into(),
            custom_text_muted: "#A0A0A0".into(),
            custom_accent: "#12345".into(),
            custom_background: " #0f0 ".into(),
            ..Settings::default()
        }
        .normalized();

        assert_eq!(settings.custom_text, Settings::default().custom_text);
        assert_eq!(settings.custom_text_muted, "#a0a0a0");
        // Short hex is not accepted: it is far more likely to be a mistake than
        // an intent, and expanding it silently would hide the typo.
        assert_eq!(settings.custom_accent, Settings::default().custom_accent);
        assert_eq!(
            settings.custom_background,
            Settings::default().custom_background
        );
    }

    #[test]
    fn background_image_never_carries_a_directory_across() {
        let settings = Settings {
            custom_background_image: Some("/home/someone/Pictures/wallpapers/ridge.png".into()),
            ..Settings::default()
        }
        .normalized();

        assert_eq!(
            settings.custom_background_image.as_deref(),
            Some("ridge.png")
        );

        let blank = Settings {
            custom_background_image: Some("   ".into()),
            ..Settings::default()
        }
        .normalized();
        assert_eq!(blank.custom_background_image, None);
    }

    #[test]
    fn rel_path_is_always_slash_separated() {
        let native = PathBuf::from("albums").join("2024").join("trip");
        assert_eq!(to_rel_string(&native), "albums/2024/trip");
    }

    #[test]
    fn rel_path_of_root_is_empty() {
        assert_eq!(to_rel_string(Path::new("")), "");
    }

    #[test]
    fn library_id_is_stable_and_path_specific() {
        let a = library_id_for(Path::new("/photos"));
        let b = library_id_for(Path::new("/photos"));
        let c = library_id_for(Path::new("/other"));

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(a.len(), 16);
    }
}
