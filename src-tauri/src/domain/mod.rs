
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Library {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub created_at: i64,
    pub last_opened_at: Option<i64>,
    pub missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub name: String,
    pub rel_path: String,
    pub image_count: i64,
    pub cover_url: Option<String>,
}

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
    pub duplicate_count: i64,
    pub thumb_url: Option<String>,
}

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
    Scanning,
    Fingerprinting,
    Done,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    pub library_id: String,
    pub phase: ScanPhase,
    pub processed: u64,
    pub total: u64,
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub duplicates: u64,
    pub current: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorderResult {
    pub folder_id: i64,
    pub positions: Vec<AssetPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetPosition {
    pub asset_id: i64,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveResult {
    pub asset_id: i64,
    pub from_folder_id: i64,
    pub to_folder_id: i64,
    pub filename: String,
    pub rel_path: String,
    pub renamed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub theme: String,
    pub interface_scale: f64,
    pub gif_autoplay: bool,
    pub max_playing_gifs: i64,
    pub folder_card_size: i64,
    pub hide_duplicates: bool,
    pub import_folder: Option<String>,
    pub import_ask: bool,
    pub thumbnail_cpu_limit: i64,
    pub custom_text: String,
    pub custom_text_muted: String,
    pub custom_accent: String,
    pub custom_background: String,
    pub custom_background_image: Option<String>,
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
            custom_text: "#ececed".to_string(),
            custom_text_muted: "#9a9aa2".to_string(),
            custom_accent: "#60a5fa".to_string(),
            custom_background: "#101012".to_string(),
            custom_background_image: None,
            custom_background_blur: 12.0,
        }
    }
}

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

        if self.thumbnail_cpu_limit < 0 {
            self.thumbnail_cpu_limit = 0;
        }
        self.thumbnail_cpu_limit = self.thumbnail_cpu_limit.min(32);

        let defaults = Settings::default();
        self.custom_text = normalize_hex(&self.custom_text, &defaults.custom_text);
        self.custom_text_muted = normalize_hex(&self.custom_text_muted, &defaults.custom_text_muted);
        self.custom_accent = normalize_hex(&self.custom_accent, &defaults.custom_accent);
        self.custom_background = normalize_hex(&self.custom_background, &defaults.custom_background);

        if !self.custom_background_blur.is_finite() {
            self.custom_background_blur = defaults.custom_background_blur;
        }
        self.custom_background_blur = self.custom_background_blur.clamp(0.0, 40.0);

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryTotals {
    pub images: i64,
    pub folders: i64,
    pub duplicate_groups: i64,
    pub redundant: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibrarySummary {
    pub id: String,
    pub images: i64,
    pub folders: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Stats {
    pub libraries: i64,
    pub unreadable: i64,
    pub images: i64,
    pub folders: i64,
    pub duplicate_groups: i64,
    pub redundant: i64,
    pub library_bytes: i64,
    pub thumbnails_bytes: i64,
    pub index_bytes: i64,
    pub cache_location: String,
}

pub fn library_id_for(root: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    hex::encode(&hasher.finalize()[..8])
}

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
        assert_eq!(settings.custom_accent, Settings::default().custom_accent);
        assert_eq!(settings.custom_background, Settings::default().custom_background);
    }

    #[test]
    fn background_image_never_carries_a_directory_across() {
        let settings = Settings {
            custom_background_image: Some("/home/someone/Pictures/wallpapers/ridge.png".into()),
            ..Settings::default()
        }
        .normalized();

        assert_eq!(settings.custom_background_image.as_deref(), Some("ridge.png"));

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