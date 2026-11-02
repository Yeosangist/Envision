
use crate::domain::{BrowserProfile, ImportProgress, ImportValidation, ImportPhase};
use sha2::{Digest, Sha256};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc::{self, Receiver, Sender}};
use std::thread;
use thiserror::Error;

/// Import-specific errors.
#[derive(Debug, Error)]
pub enum ImportError {
    #[error("Invalid Pinterest URL: {0}")]
    InvalidUrl(String),

    #[error("gallery-dl is not available")]
    GalleryDlUnavailable,

    #[error("Parent directory does not exist: {0}")]
    ParentNotFound(String),

    #[error("Parent path is not a directory: {0}")]
    NotADirectory(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Path error: {0}")]
    Path(String),
}

/// Validate and extract board ID from a Pinterest URL.
pub fn validate_pinterest_url(url: &str) -> Result<String, ImportError> {
    let url = url.trim();

    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(ImportError::InvalidUrl(
            "URL must start with http:// or https://".to_string(),
        ));
    }

    let parsed = url::Url::parse(url).map_err(|_| {
        ImportError::InvalidUrl("Could not parse URL".to_string())
    })?;

    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let is_pinterest_host = host == "pinterest.com"
        || host.ends_with(".pinterest.com")
        || host == "pinterest.co.uk"
        || host.ends_with(".pinterest.co.uk");
    if !is_pinterest_host {
        return Err(ImportError::InvalidUrl(
            "URL must be a Pinterest board URL".to_string(),
        ));
    }

    // Extract path components: /username/boardname
    let path = parsed.path();
    let parts: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if parts.len() != 2 || parts[0].eq_ignore_ascii_case("pin") {
        return Err(ImportError::InvalidUrl(
            "URL must point to a Pinterest board, not a pin or section".to_string(),
        ));
    }

    // Ignore scheme, www, query, and trailing slash so board links share an archive identity.
    let canonical_host = host.strip_prefix("www.").unwrap_or(&host);
    let board_id = format!("{}{}", canonical_host, path.trim_end_matches('/'));
    Ok(board_id)
}

/// Sanitize a board title for use as a directory name.
pub fn sanitize_folder_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| *c != '\0')
        .map(|c| {
            if c == '/' || c == '\\' {
                '-'
            } else {
                c
            }
        })
        .collect();

    let sanitized = sanitized.trim().trim_matches('.').to_string();

    // Fall back to a default name if empty after sanitization
    if sanitized.is_empty() {
        "pinterest-board".to_string()
    } else {
        sanitized
    }
}

/// Resolve a collision-safe folder name under a parent directory.
pub fn resolve_folder_name(parent: &Path, base_name: &str) -> PathBuf {
    let mut candidate = parent.join(base_name);

    if !candidate.exists() {
        return candidate;
    }

    // Try numbered suffixes: board (2), board (3), etc.
    for i in 2..1000 {
        let name = format!("{} ({})", base_name, i);
        candidate = parent.join(&name);
        if !candidate.exists() {
            return candidate;
        }
    }

    // Fallback: use a timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    parent.join(format!("{}-{}", base_name, timestamp))
}

/// Path to the import data directory in app data.
pub fn import_data_dir() -> Result<PathBuf, ImportError> {
    let data_dir = dirs::data_local_dir()
        .ok_or_else(|| ImportError::Path("Could not find app data directory".to_string()))?;
    
    let envision_dir = data_dir.join("envision");
    let import_dir = envision_dir.join("import");
    
    std::fs::create_dir_all(&import_dir)?;
    Ok(import_dir)
}

/// Path to the archive file for a specific board and parent destination.
pub fn archive_path(board_id: &str, parent_path: &Path) -> Result<PathBuf, ImportError> {
    let import_dir = import_data_dir()?;
    
    // Create a stable key from board_id + parent
    let mut hasher = Sha256::new();
    hasher.update(board_id.as_bytes());
    hasher.update(parent_path.to_string_lossy().as_bytes());
    let result = hasher.finalize();
    let key = hex::encode(result.as_slice());
    
    Ok(import_dir.join(format!("archive-{}.txt", key)))
}

/// Path to the board mapping file.
pub fn mapping_path() -> Result<PathBuf, ImportError> {
    let import_dir = import_data_dir()?;
    Ok(import_dir.join("board-mappings.json"))
}

/// Board-to-folder mapping record.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BoardMapping {
    pub board_id: String,
    pub parent_path: String,
    pub board_path: String,
    pub board_name: String,
    pub created_at: i64,
}

/// Load existing board mappings from app data.
pub fn load_mappings() -> Result<Vec<BoardMapping>, ImportError> {
    let path = mapping_path()?;
    
    if !path.exists() {
        return Ok(Vec::new());
    }
    
    let content = std::fs::read_to_string(&path)?;
    let mappings: Vec<BoardMapping> = serde_json::from_str(&content)
        .map_err(|e| ImportError::Path(format!("Failed to parse mappings: {}", e)))?;
    
    Ok(mappings)
}

/// Save board mappings to app data.
pub fn save_mappings(mappings: &[BoardMapping]) -> Result<(), ImportError> {
    let path = mapping_path()?;
    let content = serde_json::to_string_pretty(mappings)
        .map_err(|e| ImportError::Path(format!("Failed to serialize mappings: {}", e)))?;
    
    std::fs::write(&path, content)?;
    Ok(())
}

/// Find an existing mapping for a board and parent.
pub fn find_mapping(board_id: &str, parent_path: &Path) -> Option<BoardMapping> {
    let mappings = load_mappings().ok()?;
    let parent_str = parent_path.to_string_lossy().to_string();
    
    mappings
        .into_iter()
        .find(|m| m.board_id == board_id && m.parent_path == parent_str)
}

/// Add or update a board mapping.
pub fn upsert_mapping(mapping: BoardMapping) -> Result<(), ImportError> {
    let mut mappings = load_mappings()?;
    
    // Remove any existing mapping for this board + parent
    let parent_str = mapping.parent_path.clone();
    mappings.retain(|m| !(m.board_id == mapping.board_id && m.parent_path == parent_str));
    
    // Add the new mapping
    mappings.push(mapping);
    
    save_mappings(&mappings)
}

/// Validate an import request and resolve the destination.
pub fn validate_import(
    url: &str,
    parent_path: &Path,
    board_name_override: Option<&str>,
) -> Result<ImportValidation, ImportError> {
    // Validate the URL
    let board_id = validate_pinterest_url(url)?;
    
    // Validate the parent directory
    if !parent_path.exists() {
        return Err(ImportError::ParentNotFound(
            parent_path.to_string_lossy().to_string(),
        ));
    }
    
    if !parent_path.is_dir() {
        return Err(ImportError::NotADirectory(
            parent_path.to_string_lossy().to_string(),
        ));
    }
    
    // Check for existing mapping
    let is_reimport;
    let board_path;
    let board_name;
    
    if let Some(mapping) = find_mapping(&board_id, parent_path) {
        // Re-import to existing folder
        is_reimport = true;
        board_path = PathBuf::from(&mapping.board_path);
        board_name = mapping.board_name;
    } else {
        // New import: derive folder name
        is_reimport = false;
        
        // Extract board name from URL or use override
        let raw_name = if let Some(name) = board_name_override {
            name.to_string()
        } else {
            // Try to extract from URL path
            let parsed = url::Url::parse(url).unwrap();
            let path = parsed.path();
            let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            parts.last().unwrap_or(&"pinterest-board").to_string()
        };
        
        board_name = sanitize_folder_name(&raw_name);
        board_path = resolve_folder_name(parent_path, &board_name);
    }
    
    Ok(ImportValidation {
        board_id,
        board_name,
        parent_path: parent_path.to_string_lossy().to_string(),
        board_path: board_path.to_string_lossy().to_string(),
        is_reimport,
    })
}

/// Handle for an in-progress import, supporting cancellation.
#[derive(Clone)]
pub struct ImportHandle {
    cancel: Arc<AtomicBool>,
    processed: Arc<AtomicU64>,
}

impl ImportHandle {
    pub fn new() -> Self {
        Self {
            cancel: Arc::new(AtomicBool::new(false)),
            processed: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    pub fn increment_processed(&self) {
        self.processed.fetch_add(1, Ordering::SeqCst);
    }

    pub fn get_processed(&self) -> u64 {
        self.processed.load(Ordering::SeqCst)
    }
}

#[derive(Default)]
pub struct ImportTaskState {
    active: Mutex<Option<ImportHandle>>,
}

impl ImportTaskState {
    pub fn set_active(&self, handle: ImportHandle) {
        if let Ok(mut active) = self.active.lock() {
            *active = Some(handle);
        }
    }

    pub fn cancel(&self) {
        if let Ok(active) = self.active.lock() {
            if let Some(handle) = active.as_ref() {
                handle.cancel();
            }
        }
    }
}

/// Build gallery-dl command arguments for a Pinterest board import.
pub fn build_gallery_dl_args(
    url: &str,
    output_dir: &Path,
    archive_path: &Path,
    browser_profile: &BrowserProfile,
) -> Vec<String> {
    let mut args = vec![
        url.to_string(),
        "--destination".to_string(),
        output_dir.to_string_lossy().to_string(),
        "--download-archive".to_string(),
        archive_path.to_string_lossy().to_string(),
        "--sleep-request".to_string(),
        "1-2".to_string(),
        "--sleep".to_string(),
        "0.5-1.5".to_string(),
        "--sleep-429".to_string(),
        "60".to_string(),
        // Keep ordinary pins at the board root and place section pins beneath their section title.
        "--option".to_string(),
        "extractor.pinterest.board.directory=[]".to_string(),
        "--option".to_string(),
        r#"extractor.pinterest.section.directory=["{section[title]}"]"#.to_string(),
        "--filename".to_string(),
        "{num}_{id}.{extension}".to_string(),
    ];

    // Add browser cookie arguments if a profile is selected
    match browser_profile {
        BrowserProfile::Firefox(profile) => {
            args.push("--cookies-from-browser".to_string());
            args.push(browser_cookie_spec("firefox", profile));
        }
        BrowserProfile::Chrome(profile) => {
            args.push("--cookies-from-browser".to_string());
            args.push(browser_cookie_spec("chrome", profile));
        }
        BrowserProfile::Chromium(profile) => {
            args.push("--cookies-from-browser".to_string());
            args.push(browser_cookie_spec("chromium", profile));
        }
        BrowserProfile::NoCookies => {
        // No cookies; public boards only
        }
    }

    args
}

fn browser_cookie_spec(browser: &str, profile: &str) -> String {
    if profile.is_empty() {
        browser.to_string()
    } else {
        format!("{}:{}", browser, profile)
    }
}

/// Run gallery-dl as a child process and stream progress.
pub fn run_gallery_dl(
    url: &str,
    output_dir: &Path,
    archive_path: &Path,
    browser_profile: &BrowserProfile,
    handle: ImportHandle,
    progress_sender: Sender<ImportProgress>,
) -> Result<(), ImportError> {
    let args = build_gallery_dl_args(url, output_dir, archive_path, browser_profile);

    let mut child = std::process::Command::new("gallery-dl")
        .args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| ImportError::GalleryDlUnavailable)?;

    // Send initial progress
    let _ = progress_sender.send(ImportProgress {
        phase: ImportPhase::Downloading,
        processed: 0,
        total: None,
        current: None,
        error: None,
    });

    // Read stdout for progress information
    let stdout = child.stdout.take().ok_or_else(|| {
        ImportError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to capture stdout",
        ))
    })?;

    let reader = std::io::BufReader::new(stdout);
    for line in reader.lines() {
        if handle.is_cancelled() {
            child.kill()?;
            let _ = progress_sender.send(ImportProgress {
                phase: ImportPhase::Cancelled,
                processed: handle.get_processed(),
                total: None,
                current: None,
                error: None,
            });
            return Ok(());
        }

        match line {
            Ok(line) => {
                // Parse gallery-dl output for progress
                // gallery-dl outputs: "[#...] Downloading image #123..."
                if line.contains("Downloading") {
                    handle.increment_processed();
                    let _ = progress_sender.send(ImportProgress {
                        phase: ImportPhase::Downloading,
                        processed: handle.get_processed(),
                        total: None,
                        current: Some(line.clone()),
                        error: None,
                    });
                }
            }
            Err(_) => break,
        }
    }

    let status = child.wait()?;
    
    if status.success() {
        let _ = progress_sender.send(ImportProgress {
            phase: ImportPhase::Done,
            processed: handle.get_processed(),
            total: None,
            current: None,
            error: None,
        });
        Ok(())
    } else {
        let error_msg = format!("gallery-dl exited with status: {}", status);
        let _ = progress_sender.send(ImportProgress {
            phase: ImportPhase::Failed,
            processed: handle.get_processed(),
            total: None,
            current: None,
            error: Some(error_msg.clone()),
        });
        Err(ImportError::Io(std::io::Error::new(
            std::io::ErrorKind::Other,
            error_msg,
        )))
    }
}

/// Start a gallery-dl import in a background thread.
pub fn start_import(
    url: String,
    output_dir: PathBuf,
    archive_path: PathBuf,
    browser_profile: BrowserProfile,
) -> Result<(ImportHandle, Receiver<ImportProgress>), ImportError> {
    let handle = ImportHandle::new();
    let handle_clone = handle.clone();
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let result = run_gallery_dl(
            &url,
            &output_dir,
            &archive_path,
            &browser_profile,
            handle_clone,
            sender,
        );
        
        if let Err(e) = result {
            log::error!("Import failed: {}", e);
        }
    });

    Ok((handle, receiver))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_pinterest_url_accepts_valid_urls() {
        let valid_urls = [
            "https://pinterest.com/username/boardname/",
            "https://www.pinterest.com/username/boardname",
            "http://pinterest.co.uk/username/boardname/",
        ];
        
        for url in valid_urls {
            assert!(validate_pinterest_url(url).is_ok(), "URL should be valid: {}", url);
        }
    }

    #[test]
    fn validate_pinterest_url_rejects_invalid_urls() {
        let invalid_urls = [
            "not-a-url",
            "https://example.com/board",
            "https://pinterest.com/username", // Missing board name
            "https://pinterest.com.evil.example/username/board",
            "https://evilpinterest.com/username/board",
            "https://pinterest.com/pin/12345",
            "https://pinterest.com/username/board/section",
            "ftp://pinterest.com/username/board",
        ];
        
        for url in invalid_urls {
            assert!(validate_pinterest_url(url).is_err(), "URL should be invalid: {}", url);
        }
    }

    #[test]
    fn sanitize_folder_name_removes_invalid_chars() {
        assert_eq!(sanitize_folder_name("My Board"), "My Board");
        assert_eq!(sanitize_folder_name("Board/Name"), "Board-Name");
        assert_eq!(sanitize_folder_name("  Board  "), "Board");
        assert_eq!(sanitize_folder_name(".Board."), "Board");
        assert_eq!(sanitize_folder_name(""), "pinterest-board");
    }

    #[test]
    fn pinterest_board_identity_ignores_www_scheme_and_trailing_slash() {
        let canonical = validate_pinterest_url("https://pinterest.com/user/board").unwrap();
        assert_eq!(canonical, "pinterest.com/user/board");
        assert_eq!(
            validate_pinterest_url("http://www.pinterest.com/user/board/?source=share").unwrap(),
            canonical
        );
    }

    #[test]
    fn import_validation_serializes_ipc_fields_as_camel_case() {
        let validation = ImportValidation {
            board_id: "board-id".to_string(),
            board_name: "board".to_string(),
            parent_path: "/tmp".to_string(),
            board_path: "/tmp/board".to_string(),
            is_reimport: false,
        };

        let value = serde_json::to_value(validation).unwrap();
        assert_eq!(value["parentPath"], "/tmp");
        assert_eq!(value["boardPath"], "/tmp/board");
        assert_eq!(value["isReimport"], false);
        assert!(value.get("parent_path").is_none());
    }

    #[test]
    fn browser_selection_builds_gallery_dl_cookie_arguments() {
        let args = build_gallery_dl_args(
            "https://pinterest.com/user/board/",
            Path::new("/tmp/board"),
            Path::new("/tmp/archive.txt"),
            &BrowserProfile::from_string(Some("firefox".to_string())),
        );

        assert!(args.windows(2).any(|pair| {
            pair == ["--cookies-from-browser", "firefox"]
        }));
        assert!(!args.iter().any(|arg| arg == "--cookies"));
    }

    #[test]
    fn command_arguments_include_conservative_rate_limits() {
        let args = build_gallery_dl_args(
            "https://pinterest.com/user/board/",
            Path::new("/tmp/board"),
            Path::new("/tmp/archive.txt"),
            &BrowserProfile::NoCookies,
        );

        for expected in [
            ["--sleep-request", "1-2"],
            ["--sleep", "0.5-1.5"],
            ["--sleep-429", "60"],
        ] {
            assert!(args.windows(2).any(|pair| pair == expected));
        }
    }

    #[test]
    fn command_arguments_map_pinterest_sections_to_folders() {
        let args = build_gallery_dl_args(
            "https://pinterest.com/user/board/",
            Path::new("/tmp/board"),
            Path::new("/tmp/archive.txt"),
            &BrowserProfile::NoCookies,
        );

        assert!(args.windows(2).any(|pair| pair == ["--destination", "/tmp/board"]));
        assert!(args.iter().any(|arg| arg == "extractor.pinterest.board.directory=[]"));
        assert!(args.iter().any(|arg| {
            arg == r#"extractor.pinterest.section.directory=["{section[title]}"]"#
        }));
        assert!(args.iter().any(|arg| arg == "{num}_{id}.{extension}"));
        assert!(!args.iter().any(|arg| arg == "--directory"));
    }
}
