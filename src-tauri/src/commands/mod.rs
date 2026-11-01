
//! Tauri command boundary: validate requests, call services, and return domain types.
use crate::database::{delete_setting, read_setting, write_setting, APP_MIGRATIONS};
use crate::domain::{Asset, DuplicateGroup, Folder, Library, LibrarySummary, MoveResult, ReorderResult, ScanProgress, ScanPhase, Settings, Stats};
use crate::library::scan::{ScanHandle, ScanOptions, ScanOutcome};
use crate::library::{self, AppState, LibraryError, LibraryIndex};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

/// Event carrying scan progress.
pub const SCAN_EVENT: &str = "scan:progress";

/// A categorised failure safe to display in the UI.
#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct CommandError {
    pub category: String,
    pub message: String,
}

impl From<LibraryError> for CommandError {
    fn from(error: LibraryError) -> Self {
        let category = match &error {
            LibraryError::NoOpenLibrary => "no_library",
            LibraryError::NotFound(_) | LibraryError::FolderNotFound(_) | LibraryError::AssetNotFound(_) => "not_found",
            LibraryError::Invalid(_) => "invalid",
            LibraryError::Path(_) => "invalid_path",
            LibraryError::Io(_) => "io",
            LibraryError::Platform(_) => "platform",
            LibraryError::Thumbnail(_) => "thumbnail",
            LibraryError::Database(_) | LibraryError::Sqlite(_) => "database",
        };
        Self {
            category: category.to_string(),
            message: error.to_string(),
        }
    }
}

impl From<crate::database::DatabaseError> for CommandError {
    fn from(error: crate::database::DatabaseError) -> Self {
        Self {
            category: "database".into(),
            message: error.to_string(),
        }
    }
}

impl From<crate::platform::PlatformError> for CommandError {
    fn from(error: crate::platform::PlatformError) -> Self {
        Self {
            category: "platform".into(),
            message: error.to_string(),
        }
    }
}

impl From<tauri_plugin_opener::Error> for CommandError {
    fn from(error: tauri_plugin_opener::Error) -> Self {
        Self {
            category: "external_handler".into(),
            message: error.to_string(),
        }
    }
}

impl From<crate::stats::StatsError> for CommandError {
    fn from(error: crate::stats::StatsError) -> Self {
        let category = match &error {
            crate::stats::StatsError::Sqlite(_) => "database",
            crate::stats::StatsError::Io(_) => "io",
        };
        Self {
            category: category.to_string(),
            message: error.to_string(),
        }
    }
}

pub type Result<T> = std::result::Result<T, CommandError>;


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddLibraryRequest {
    pub path: String,
    pub name: Option<String>,
}

#[tauri::command]
pub async fn add_library(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    request: AddLibraryRequest,
) -> Result<Library> {
    let library = state.with_app_db(|conn| {
        library::add_library(conn, request.name.as_deref(), &PathBuf::from(&request.path))
    })?;

    crate::platform::ensure_library_dirs(&app, &library.id)?;
    Ok(library)
}

#[tauri::command]
pub async fn list_libraries(state: State<'_, Arc<AppState>>) -> Result<Vec<Library>> {
    state.with_app_db(library::list_libraries).map_err(Into::into)
}

#[tauri::command]
pub async fn open_library(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    library_id: String,
) -> Result<Library> {
    let library = state.open_library(&library_id)?;

    if let Ok(handle) = library::watcher::start(
        app.clone(),
        library.id.clone(),
        PathBuf::from(&library.root_path),
    ) {
        state.install_watcher(handle);
    }

    if let Ok(thumbnails) = state.with_index(|index| Ok(index.thumbnails_dir.clone())) {
        if let Err(error) = app.asset_protocol_scope().allow_directory(&thumbnails, true) {
            log::warn!("could not scope the thumbnail cache: {error}");
        }
    }

    Ok(library)
}

#[tauri::command]
pub async fn close_library(state: State<'_, Arc<AppState>>) -> Result<()> {
    state.close_library();
    Ok(())
}

/// Forget a library and remove its derived data without touching user files.
#[tauri::command]
pub async fn forget_library(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    library_id: String,
) -> Result<()> {
    if state.open_library_id().as_deref() == Some(library_id.as_str()) {
        state.close_library();
    }
    state
        .with_app_db(|conn| library::forget_library(&app, conn, &library_id))
        .map_err(Into::into)
}

/// Point an existing library at a folder that has since moved.
#[tauri::command]
pub async fn relink_library(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
    library_id: String,
    path: String,
) -> Result<Library> {
    let was_open = state.open_library_id().as_deref() == Some(library_id.as_str());
    let library = state.with_app_db(|conn| {
        library::relink_library(conn, &library_id, &PathBuf::from(&path))
    })?;

    crate::platform::ensure_library_dirs(&app, &library.id)?;

    if was_open {
        state.open_library(&library_id)?;
    }
    Ok(library)
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanReport {
    pub library_id: String,
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
    pub duplicates: u64,
    pub fingerprinted: u64,
    pub folders: u64,
}

#[derive(Clone, serde::Serialize)]
struct ThumbnailReadyEvent {
    library_id: String,
    asset_id: i64,
}

fn emit(app: &AppHandle, progress: ScanProgress) {
    let _ = app.emit(SCAN_EVENT, progress);
}

/// Reconcile the open library, then fingerprint changed files in the background.
#[tauri::command]
pub async fn scan_library(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<ScanReport> {
    let library_id = state
        .open_library_id()
        .ok_or_else(|| CommandError::from(LibraryError::NoOpenLibrary))?;
    let root = state
        .with_index(|index| Ok(index.root.clone()))
        .map_err(CommandError::from)?;
    let thumbnail_cpu_limit = configured_thumbnail_cpu_limit(&state);

    let cancel: ScanHandle = library::fingerprint::Cancel::new();
    state.set_scan(cancel.clone());

    emit(
        &app,
        ScanProgress {
            library_id: library_id.clone(),
            phase: ScanPhase::Scanning,
            processed: 0,
            total: 0,
            added: 0,
            updated: 0,
            removed: 0,
            duplicates: 0,
            current: None,
        },
    );

    let outcome: ScanOutcome = state
        .with_index(|index| library::scan::reconcile_index(index, ScanOptions::default(), &cancel))
        .map_err(CommandError::from)?;

    let report = ScanReport {
        library_id: library_id.clone(),
        added: outcome.added,
        updated: outcome.updated,
        removed: outcome.removed,
        duplicates: outcome.duplicates,
        fingerprinted: 0,
        folders: outcome.folders,
    };

    // Fingerprinting uses a separate connection on a blocking thread.
    let fingerprint_app = app.clone();
    let fingerprint_id = library_id;
    tauri::async_runtime::spawn_blocking(move || {
        let app = fingerprint_app;
        // Whatever happens below, the UI gets exactly one terminal event, so it
        // can never be left showing a scan that has in fact stopped.
        let index = match LibraryIndex::open(&app, &fingerprint_id, root) {
            Ok(index) => index,
            Err(error) => {
                log::error!("could not open the index for fingerprinting: {error}");
                state_clear_scan(&app);
                emit(
                    &app,
                    ScanProgress {
                        library_id: fingerprint_id,
                        phase: ScanPhase::Failed,
                        processed: 0,
                        total: 0,
                        added: 0,
                        updated: 0,
                        removed: 0,
                        duplicates: 0,
                        current: None,
                    },
                );
                return;
            }
        };

        let pending = library::fingerprint::collect_pending(&index).unwrap_or_default();
        let total = pending.len() as u64;

        if total > 0 {
            let progress_app = app.clone();
            let progress_id = fingerprint_id.clone();
            let outcome = library::fingerprint::fingerprint_all(
                &index,
                pending,
                default_concurrency(),
                &cancel,
                move |done, total, current| {
                    emit(
                        &progress_app,
                        ScanProgress {
                            library_id: progress_id.clone(),
                            phase: ScanPhase::Fingerprinting,
                            processed: done,
                            total,
                            added: 0,
                            updated: 0,
                            removed: 0,
                            duplicates: 0,
                            current: current
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string()),
                        },
                    );
                },
            );

            if let Ok(outcome) = outcome {
                log::info!(
                    "fingerprinted {} files in {:?} ({} vanished)",
                    outcome.fingerprinted, outcome.elapsed, outcome.missing
                );
            }
        }

        // Fingerprints make duplicate detection possible, so report the totals
        // the UI shows in its header and duplicates view.
        let (_, redundant) = library::queries::duplicate_summary(&index).unwrap_or((0, 0));

        // Generate missing thumbnails in the background as a fallback to folder-prioritized work.
        let index_id = index.id.clone();
        let event_library_id = index_id.clone();
        let index_root = index.root.clone();
        let index_thumbs = index.thumbnails_dir.clone();
        drop(index);
        // Spawn progress emitter
        let app_clone = app.clone();
        std::thread::spawn(move || {
            // Reopen index in background thread
            if let Ok(index_path) = crate::platform::library_index_path(&app_clone, &index_id) {
                if let Ok(conn) = crate::database::open(&index_path, crate::database::LIBRARY_MIGRATIONS) {
                    let bg_index = LibraryIndex {
                        id: index_id,
                        root: index_root,
                        conn,
                        thumbnails_dir: index_thumbs,
                    };
                    generate_thumbnails_with_callback(
                        &bg_index,
                        thumbnail_cpu_limit,
                        |asset_id| {
                            let _ = app_clone.emit(
                                "thumbnails:ready",
                                ThumbnailReadyEvent {
                                    library_id: event_library_id.clone(),
                                    asset_id,
                                },
                            );
                        },
                    );
                }
            }
        });
        state_clear_scan(&app);
        emit(
            &app,
            ScanProgress {
                library_id: fingerprint_id,
                phase: if cancel.is_cancelled() {
                    ScanPhase::Cancelled
                } else {
                    ScanPhase::Done
                },
                processed: total,
                total,
                added: 0,
                updated: 0,
                removed: 0,
                duplicates: redundant as u64,
                current: None,
            },
        );
    });

    Ok(report)
}

fn state_clear_scan(app: &AppHandle) {
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        state.clear_scan();
    }
}

/// Render a thumbnail for every decodable asset that does not have one yet.
fn generate_thumbnails_with_callback(
    index: &LibraryIndex,
    cpu_limit: i64,
    on_ready: impl Fn(i64) + Sync,
) {
    generate_thumbnails_for_query(
        index,
        "SELECT id, rel_path, sha256 FROM assets WHERE decode_ok = 1 ORDER BY id",
        cpu_limit,
        on_ready,
    );
}

fn generate_thumbnails_for_folder_with_callback(
    index: &LibraryIndex,
    folder_id: i64,
    cpu_limit: i64,
    on_ready: impl Fn(i64) + Sync,
) {
    let sql = format!("SELECT id, rel_path, sha256 FROM assets WHERE decode_ok = 1 AND folder_id = {} ORDER BY position, id", folder_id);
    generate_thumbnails_for_query(index, &sql, cpu_limit, on_ready);
}

pub fn generate_thumbnails_for_assets(index: &LibraryIndex, asset_ids: &[i64], cpu_limit: i64) {
    if asset_ids.is_empty() {
        return;
    }
    let placeholders: String = (0..asset_ids.len()).map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("SELECT id, rel_path, sha256 FROM assets WHERE decode_ok = 1 AND id IN ({})", placeholders);
    // Build params
    let mut stmt = match index.conn.prepare(&sql) {
        Ok(stmt) => stmt,
        Err(error) => {
            log::error!("could not list images for thumbnails: {error}");
            return;
        }
    };
    let params: Vec<i64> = asset_ids.to_vec();
    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
    let rows = match stmt.query_map(rusqlite::params_from_iter(params_refs), |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?))).and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>() ) {
        Ok(rows) => rows,
        Err(error) => {
            log::error!("could not list images for thumbnails: {error}");
            return;
        }
    };
    generate_thumbnails_from_rows(index, rows, cpu_limit, |_| {});
}

fn generate_thumbnails_for_query(
    index: &LibraryIndex,
    sql: &str,
    cpu_limit: i64,
    on_ready: impl Fn(i64) + Sync,
) {
    let rows: Vec<(i64, String, Option<String>)> = {
        let mut stmt = match index.conn.prepare(sql) {
            Ok(stmt) => stmt,
            Err(error) => {
                log::error!("could not list images for thumbnails: {error}");
                return;
            }
        };
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .and_then(|mapped| mapped.collect::<rusqlite::Result<Vec<_>>>());
        match rows {
            Ok(rows) => rows,
            Err(error) => {
                log::error!("could not list images for thumbnails: {error}");
                return;
            }
        }
    };
    generate_thumbnails_from_rows(index, rows, cpu_limit, on_ready);
}

/// Read the thumbnail CPU limit, defaulting to all available cores on failure.
fn configured_thumbnail_cpu_limit(state: &State<'_, Arc<AppState>>) -> i64 {
    state
        .with_app_db(|conn| read_settings(conn).map_err(LibraryError::from))
        .map(|settings| settings.thumbnail_cpu_limit)
        .unwrap_or(0)
}

fn generate_thumbnails_from_rows(
    index: &LibraryIndex,
    rows: Vec<(i64, String, Option<String>)>,
    cpu_limit: i64,
    on_ready: impl Fn(i64) + Sync,
) {
    use rayon::prelude::*;

    let started = std::time::Instant::now();
    let made = std::sync::atomic::AtomicU64::new(0);
    let failed = std::sync::atomic::AtomicU64::new(0);
    let cached = std::sync::atomic::AtomicU64::new(0);

    // Extract thread-safe data before parallel iteration
    let thumbnails_dir = index.thumbnails_dir.clone();
    let root = index.root.clone();

    crate::thumbnails::with_worker_pool(cpu_limit, || {
        rows.par_iter().for_each(|(id, rel_path, sha256)| {
            let key = library::queries::thumbnail_key(*id, sha256.as_deref());
            if crate::thumbnails::is_cached(&thumbnails_dir, &key) {
                cached.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return;
            }

            // Stored paths are untrusted; re-derive before opening anything.
            let source = match crate::paths::resolve_within(&root, rel_path) {
                Ok(path) => path,
                Err(_) => return,
            };

            match crate::thumbnails::ensure(&thumbnails_dir, &key, &source, true) {
                Ok(Some(_)) => {
                    made.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    on_ready(*id);
                }
                Ok(None) => {}
                Err(error) => {
                    let count = failed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if count < 3 {
                        log::warn!("thumbnail for {rel_path} failed: {error}");
                    }
                }
            }
        });
    });

    let made = made.load(std::sync::atomic::Ordering::Relaxed);
    let failed = failed.load(std::sync::atomic::Ordering::Relaxed);
    let cached_count = cached.load(std::sync::atomic::Ordering::Relaxed);
    let total = rows.len() as u64;

    if made > 0 || failed > 0 {
        log::info!(
            "{} thumbnails written, {} cached, {} failed ({} total) in {:?}",
            made,
            cached_count,
            failed,
            total,
            started.elapsed()
        );
    }
}

fn default_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get().clamp(1, 8))
        .unwrap_or(2)
}

#[tauri::command]
pub async fn cancel_scan(state: State<'_, Arc<AppState>>) -> Result<()> {
    state.with_scan(|cancel| cancel.cancel());
    Ok(())
}

/// Whether a scan is currently in flight for the open library.
#[tauri::command]
pub async fn scan_status(state: State<'_, Arc<AppState>>) -> Result<bool> {
    Ok(state.has_scan())
}

#[tauri::command]
pub async fn generate_thumbnails_for_folder_cmd(
    state: State<'_, Arc<AppState>>,
    folder_id: i64,
) -> Result<()> {
    let index_id = state.with_index(|index| Ok(index.id.clone()))?;
    let index_root = state.with_index(|index| Ok(index.root.clone()))?;
    let index_thumbs = state.with_index(|index| Ok(index.thumbnails_dir.clone()))?;
    let cpu_limit = configured_thumbnail_cpu_limit(&state);
    let app = state.app.clone();
    let event_library_id = index_id.clone();
    std::thread::spawn(move || {
        if let Ok(index_path) = crate::platform::library_index_path(&app, &index_id) {
            if let Ok(conn) = crate::database::open(&index_path, crate::database::LIBRARY_MIGRATIONS) {
                let bg_index = LibraryIndex {
                    id: index_id,
                    root: index_root,
                    conn,
                    thumbnails_dir: index_thumbs,
                };
                generate_thumbnails_for_folder_with_callback(
                    &bg_index,
                    folder_id,
                    cpu_limit,
                    |asset_id| {
                        let _ = app.emit(
                            "thumbnails:ready",
                            ThumbnailReadyEvent {
                                library_id: event_library_id.clone(),
                                asset_id,
                            },
                        );
                    },
                );
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn generate_thumbnails_for_assets_cmd(
    state: State<'_, Arc<AppState>>,
    asset_ids: Vec<i64>,
) -> Result<()> {
    if asset_ids.is_empty() {
        return Ok(());
    }
    let index_id = state.with_index(|index| Ok(index.id.clone()))?;
    let index_root = state.with_index(|index| Ok(index.root.clone()))?;
    let index_thumbs = state.with_index(|index| Ok(index.thumbnails_dir.clone()))?;
    let cpu_limit = configured_thumbnail_cpu_limit(&state);
    let app = state.app.clone();
    std::thread::spawn(move || {
        if let Ok(index_path) = crate::platform::library_index_path(&app, &index_id) {
            if let Ok(conn) = crate::database::open(&index_path, crate::database::LIBRARY_MIGRATIONS) {
                let bg_index = LibraryIndex {
                    id: index_id,
                    root: index_root,
                    conn,
                    thumbnails_dir: index_thumbs,
                };
                generate_thumbnails_for_assets(&bg_index, &asset_ids, cpu_limit);
            }
        }
    });
    Ok(())
}


#[tauri::command]
pub async fn list_child_folders(
    state: State<'_, Arc<AppState>>,
    parent_id: Option<i64>,
) -> Result<Vec<Folder>> {
    state
        .with_index(|index| library::list_child_folders(index, parent_id))
        .map_err(Into::into)
}

/// List all library folders for navigation and breadcrumbs.
#[tauri::command]
pub async fn list_folders(state: State<'_, Arc<AppState>>) -> Result<Vec<Folder>> {
    state.with_index(library::list_all_folders).map_err(Into::into)
}

#[tauri::command]
pub async fn folder_ancestry(state: State<'_, Arc<AppState>>, folder_id: i64) -> Result<Vec<Folder>> {
    state
        .with_index(|index| library::folder_ancestry(index, folder_id))
        .map_err(Into::into)
}

#[tauri::command]
pub async fn list_assets(state: State<'_, Arc<AppState>>, folder_id: i64) -> Result<Vec<Asset>> {
    state
        .with_index(|index| library::list_assets(index, folder_id))
        .map_err(Into::into)
}

#[tauri::command]
pub async fn reorder_assets(
    state: State<'_, Arc<AppState>>,
    folder_id: i64,
    ordered_ids: Vec<i64>,
) -> Result<ReorderResult> {
    state
        .with_index(|index| library::reorder_assets(index, folder_id, &ordered_ids))
        .map_err(Into::into)
}

#[tauri::command]
pub async fn move_assets(
    state: State<'_, Arc<AppState>>,
    asset_ids: Vec<i64>,
    to_folder_id: i64,
) -> Result<Vec<MoveResult>> {
    state
        .with_index(|index| library::move_assets(index, &asset_ids, to_folder_id))
        .map_err(Into::into)
}

#[tauri::command]
pub async fn create_folder(
    state: State<'_, Arc<AppState>>,
    parent_id: i64,
    name: String,
) -> Result<Folder> {
    state
        .with_index(|index| {
            let id = library::create_folder(index, parent_id, &name)?;
            library::folder_by_id(index, id)
        })
        .map_err(Into::into)
}

#[tauri::command]
pub async fn rename_folder(
    state: State<'_, Arc<AppState>>,
    folder_id: i64,
    name: String,
) -> Result<Folder> {
    state
        .with_index(|index| {
            library::rename_folder(index, folder_id, &name)?;
            library::folder_by_id(index, folder_id)
        })
        .map_err(Into::into)
}

/// Read original image bytes by asset id without exposing its filesystem path.
#[tauri::command]
pub async fn read_asset_bytes(
    state: State<'_, Arc<AppState>>,
    asset_id: i64,
) -> Result<tauri::ipc::Response> {
    let bytes = state
        .with_index(|index| {
            let asset = library::asset_by_id(index, asset_id)?;
            let path = index
                .asset_real_path(&asset.rel_path)?
                .ok_or_else(|| LibraryError::Invalid(format!("{} is not readable", asset.rel_path)))?;
            let bytes = std::fs::read(&path)?;
            Ok(bytes)
        })
        .map_err(CommandError::from)?;

    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn open_asset_externally(
    state: State<'_, Arc<AppState>>,
    asset_id: i64,
) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;

    let (path, _) = state.with_index(|index| {
        let asset = library::asset_by_id(index, asset_id)?;
        let path = index
            .asset_path(&asset.rel_path)?
            .ok_or_else(|| LibraryError::Invalid(format!("{} is not readable", asset.rel_path)))?;
        Ok((path, asset.ext))
    })?;

    state.app.opener().open_path(path.to_string_lossy().to_string(), None::<&str>)?;
    Ok(())
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateSummary {
    pub groups: i64,
    pub redundant: i64,
}

#[tauri::command]
pub async fn list_duplicates(state: State<'_, Arc<AppState>>) -> Result<Vec<DuplicateGroup>> {
    state.with_index(library::list_duplicates).map_err(Into::into)
}

#[tauri::command]
pub async fn duplicate_summary(state: State<'_, Arc<AppState>>) -> Result<DuplicateSummary> {
    state
        .with_index(|index| {
            let (groups, redundant) = library::duplicate_summary(index)?;
            Ok(DuplicateSummary { groups, redundant })
        })
        .map_err(Into::into)
}

/// Open formats Envision does not decode with the desktop's registered handler.
/// Reclaim unused thumbnail cache entries on explicit request.
#[tauri::command]
pub async fn prune_thumbnails(state: State<'_, Arc<AppState>>) -> Result<i64> {
    state
        .with_index(|index| {
            let live: Vec<String> = {
                let mut stmt = index
                    .conn
                    .prepare("SELECT DISTINCT sha256 FROM assets WHERE sha256 IS NOT NULL")?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                rows
            };
            let removed = crate::thumbnails::prune(&index.thumbnails_dir, &live)?;
            Ok(removed as i64)
        })
        .map_err(Into::into)
}


/// App-data path for the theme image; only its filename is exposed to the UI.
const THEME_BACKGROUND_KEY: &str = "theme_background_path";

fn read_settings(conn: &Connection) -> crate::database::Result<Settings> {
    let theme = read_setting(conn, "theme")?.unwrap_or_else(|| "system".into());
    let interface_scale = read_setting(conn, "interface_scale")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let gif_autoplay = read_setting(conn, "gif_autoplay")?
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);
    let max_playing_gifs = read_setting(conn, "max_playing_gifs")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(4);
    let folder_card_size = read_setting(conn, "folder_card_size")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(160);
    let hide_duplicates = read_setting(conn, "hide_duplicates")?
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);
    let import_folder = read_setting(conn, "import_folder")?.filter(|v| !v.trim().is_empty());
    let import_ask = read_setting(conn, "import_ask")?
        .map(|v| v == "true" || v == "1")
        .unwrap_or(true);
    let thumbnail_cpu_limit = read_setting(conn, "thumbnail_cpu_limit")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    // Absent colour keys read as empty and normalisation supplies the default,
    // so the palette lives in exactly one place (`Settings::default`).
    let custom_text = read_setting(conn, "custom_text")?.unwrap_or_default();
    let custom_text_muted = read_setting(conn, "custom_text_muted")?.unwrap_or_default();
    let custom_accent = read_setting(conn, "custom_accent")?.unwrap_or_default();
    let custom_background = read_setting(conn, "custom_background")?.unwrap_or_default();
    let custom_background_blur = read_setting(conn, "custom_background_blur")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(12.0);
    let custom_background_image = read_setting(conn, THEME_BACKGROUND_KEY)?
        .and_then(|path| PathBuf::from(path).file_name().map(|name| name.to_string_lossy().to_string()));

    Ok(Settings {
        theme,
        interface_scale,
        gif_autoplay,
        max_playing_gifs,
        folder_card_size,
        hide_duplicates,
        import_folder,
        import_ask,
        thumbnail_cpu_limit,
        custom_text,
        custom_text_muted,
        custom_accent,
        custom_background,
        custom_background_image,
        custom_background_blur,
    }
    .normalized())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, Arc<AppState>>) -> Result<Settings> {
    state
        .with_app_db(|conn| read_settings(conn).map_err(LibraryError::from))
        .map_err(CommandError::from)
}

/// Persist settings, excluding the background filename derived from its path.
fn persist_settings(conn: &Connection, settings: &Settings) -> crate::database::Result<()> {
    write_setting(conn, "theme", &settings.theme)?;
    write_setting(conn, "interface_scale", &settings.interface_scale.to_string())?;
    write_setting(
        conn,
        "gif_autoplay",
        if settings.gif_autoplay { "true" } else { "false" },
    )?;
    write_setting(conn, "max_playing_gifs", &settings.max_playing_gifs.to_string())?;
    write_setting(conn, "folder_card_size", &settings.folder_card_size.to_string())?;
    write_setting(
        conn,
        "hide_duplicates",
        if settings.hide_duplicates { "true" } else { "false" },
    )?;
    match &settings.import_folder {
        Some(path) if !path.trim().is_empty() => write_setting(conn, "import_folder", path)?,
        _ => delete_setting(conn, "import_folder")?,
    }
    write_setting(
        conn,
        "import_ask",
        if settings.import_ask { "true" } else { "false" },
    )?;
    write_setting(conn, "thumbnail_cpu_limit", &settings.thumbnail_cpu_limit.to_string())?;
    write_setting(conn, "custom_text", &settings.custom_text)?;
    write_setting(conn, "custom_text_muted", &settings.custom_text_muted)?;
    write_setting(conn, "custom_accent", &settings.custom_accent)?;
    write_setting(conn, "custom_background", &settings.custom_background)?;
    write_setting(
        conn,
        "custom_background_blur",
        &settings.custom_background_blur.to_string(),
    )?;
    Ok(())
}

#[tauri::command]
pub async fn update_settings(
    state: State<'_, Arc<AppState>>,
    settings: Settings,
) -> Result<Settings> {
    // Normalise before writing so what is stored always matches what the UI
    // will be asked to render.
    let normalized = settings.normalized();
    state
        .with_app_db(|conn| persist_settings(conn, &normalized).map_err(LibraryError::from))
        .map_err(CommandError::from)?;
    Ok(normalized)
}

/// Set or clear the custom background image; only its filename is returned to the UI.
#[tauri::command]
pub async fn set_theme_background(
    state: State<'_, Arc<AppState>>,
    path: Option<String>,
) -> Result<Option<String>> {
    let resolved = match path.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
        Some(chosen) => {
            let candidate = PathBuf::from(chosen);
            let canonical = candidate.canonicalize().map_err(|error| CommandError {
                category: "invalid_path".into(),
                message: format!("{}: {error}", candidate.display()),
            })?;
            if !canonical.is_file() {
                return Err(CommandError {
                    category: "invalid_path".into(),
                    message: format!("{} is not a file", canonical.display()),
                });
            }
            Some(canonical)
        }
        None => None,
    };

    state
        .with_app_db(|conn| {
            match &resolved {
                Some(path) => write_setting(conn, THEME_BACKGROUND_KEY, &path.to_string_lossy())?,
                None => delete_setting(conn, THEME_BACKGROUND_KEY)?,
            }
            Ok(())
        })
        .map_err(CommandError::from)?;

    Ok(resolved.and_then(|path| {
        path.file_name().map(|name| name.to_string_lossy().to_string())
    }))
}

/// Read the theme background as IPC bytes without exposing its path.
#[tauri::command]
pub async fn read_theme_background(
    state: State<'_, Arc<AppState>>,
) -> Result<tauri::ipc::Response> {
    let path = state
        .with_app_db(|conn| Ok(read_setting(conn, THEME_BACKGROUND_KEY)?.unwrap_or_default()))
        .map_err(CommandError::from)?;

    if path.trim().is_empty() {
        return Err(CommandError {
            category: "not_found".into(),
            message: "No custom background image is set.".into(),
        });
    }

    let bytes = std::fs::read(&path).map_err(|error| CommandError {
        category: "io".into(),
        message: format!("{path}: {error}"),
    })?;

    Ok(tauri::ipc::Response::new(bytes))
}


/// Return tracked-library totals and derived cache sizes, even with no library open.
#[tauri::command]
pub async fn library_stats(state: State<'_, Arc<AppState>>) -> Result<Stats> {
    let libraries = state
        .with_app_db(library::list_libraries)
        .map_err(CommandError::from)?;

    let cache_root = crate::platform::libraries_dir(&state.app)?;
    let mut stats = Stats {
        libraries: libraries.len() as i64,
        cache_location: cache_root.to_string_lossy().to_string(),
        ..Stats::default()
    };

    for library in &libraries {
        let dir = crate::platform::library_dir(&state.app, &library.id)?;
        let index = crate::platform::library_index_path(&state.app, &library.id)?;

        crate::stats::accumulate_index(&mut stats, &index);
        stats.thumbnails_bytes += crate::stats::dir_size(&dir.join("thumbnails"))? as i64;
        stats.index_bytes += ["index.db", "index.db-wal", "index.db-shm"]
            .iter()
            .map(|name| crate::stats::file_size(&dir.join(name)))
            .sum::<u64>() as i64;
    }

    Ok(stats)
}

/// Return cheap per-library counts for the home screen.
#[tauri::command]
pub async fn library_summaries(state: State<'_, Arc<AppState>>) -> Result<Vec<LibrarySummary>> {
    let libraries = state
        .with_app_db(library::list_libraries)
        .map_err(CommandError::from)?;

    let mut summaries = Vec::with_capacity(libraries.len());
    for library in &libraries {
        let index = crate::platform::library_index_path(&state.app, &library.id)?;
        let totals = crate::stats::summary(&index);
        summaries.push(LibrarySummary {
            id: library.id.clone(),
            images: totals.images,
            folders: totals.folders,
        });
    }

    Ok(summaries)
}

/// Applied once at startup so a fresh install has sane defaults on disk.
pub fn seed_default_settings(conn: &Connection) -> crate::database::Result<()> {
    for (key, value) in [
        ("theme", "system"),
        ("interface_scale", "1.0"),
        ("gif_autoplay", "true"),
        ("max_playing_gifs", "4"),
        ("folder_card_size", "160"),
        ("hide_duplicates", "false"),
        ("thumbnail_cpu_limit", "0"),
        ("custom_text", "#ececed"),
        ("custom_text_muted", "#9a9aa2"),
        ("custom_accent", "#60a5fa"),
        ("custom_background", "#101012"),
        ("custom_background_blur", "12"),
    ] {
        if read_setting(conn, key)?.is_none() {
            write_setting(conn, key, value)?;
        }
    }
    Ok(())
}

pub fn open_app_db(app: &AppHandle) -> crate::database::Result<Connection> {
    let path = crate::platform::app_db(app).map_err(|error| {
        crate::database::DatabaseError::Migration(error.to_string())
    })?;
    let conn = crate::database::open(&path, APP_MIGRATIONS)?;
    seed_default_settings(&conn)?;
    Ok(conn)
}

/// Reclaim thumbnail caches across registered libraries, skipping missing roots.
#[tauri::command]
pub async fn prune_all_thumbnails(state: State<'_, Arc<AppState>>) -> Result<i64> {
    let libraries = state.with_app_db(|conn| crate::library::list_libraries(conn))?;
    let mut removed_total: i64 = 0;
    for library in libraries {
        if library.missing {
            continue;
        }
        let root = match crate::platform::validate_library_root(std::path::Path::new(&library.root_path)) {
            Ok(path) => path,
            Err(_) => continue,
        };
        let index = match crate::library::LibraryIndex::open(&state.app, &library.id, root) {
            Ok(index) => index,
            Err(_) => continue,
        };
        let live: Vec<String> = match index
            .conn
            .prepare("SELECT DISTINCT sha256 FROM assets WHERE sha256 IS NOT NULL")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()
            }) {
            Ok(rows) => rows,
            Err(_) => continue,
        };
        if let Ok(removed) = crate::thumbnails::prune(&index.thumbnails_dir, &live) {
            removed_total += removed as i64;
        }
    }
    Ok(removed_total)
}


/// Check whether `gallery-dl` is available on this machine.
#[tauri::command]
pub async fn check_gallery_dl() -> Result<bool> {
    Ok(std::process::Command::new("gallery-dl")
        .arg("--version")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false))
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::open_in_memory;

    #[test]
    fn settings_round_trip_and_normalise() {
        let conn = open_in_memory(APP_MIGRATIONS).unwrap();
        seed_default_settings(&conn).unwrap();

        let defaults = read_settings(&conn).unwrap();
        assert_eq!(defaults.theme, "system");
        assert_eq!(defaults.interface_scale, 1.0);
        assert!(!defaults.hide_duplicates);
        assert_eq!(defaults.thumbnail_cpu_limit, 0, "0 means all available cores");
        assert_eq!(defaults.custom_background_image, None);

        // Out-of-range input is clamped on the way in, not on the way out.
        let stored = Settings {
            theme: "DARK".into(),
            interface_scale: 3.0,
            gif_autoplay: false,
            max_playing_gifs: 100,
            folder_card_size: 10,
            hide_duplicates: true,
            thumbnail_cpu_limit: 2,
            custom_text: "#ABCDEF".into(),
            ..Settings::default()
        }
        .normalized();
        persist_settings(&conn, &stored).unwrap();

        let read_back = read_settings(&conn).unwrap();
        assert_eq!(read_back, stored);
        // `dark` was the old name for the current palette; it reads back as
        // `grey` without needing a schema migration.
        assert_eq!(read_back.theme, "grey");
        assert_eq!(read_back.interface_scale, 1.5);
        assert_eq!(read_back.max_playing_gifs, 12);
        assert_eq!(read_back.folder_card_size, 96);
        assert!(read_back.hide_duplicates);
        assert_eq!(read_back.thumbnail_cpu_limit, 2);
        assert_eq!(read_back.custom_text, "#abcdef");

        // Normalisation is idempotent, so a re-read of the same store is stable.
        assert_eq!(read_back.clone().normalized(), read_back);
    }

    #[test]
    fn a_corrupt_stored_value_cannot_reach_the_ui() {
        let conn = open_in_memory(APP_MIGRATIONS).unwrap();
        seed_default_settings(&conn).unwrap();

        write_setting(&conn, "interface_scale", "not a number").unwrap();
        write_setting(&conn, "theme", "neon").unwrap();
        write_setting(&conn, "custom_accent", "#nope").unwrap();

        let read_back = read_settings(&conn).unwrap();
        assert_eq!(read_back.theme, "system");
        assert_eq!(read_back.interface_scale, 1.0);
        assert_eq!(read_back.custom_accent, Settings::default().custom_accent);
    }

    /// Ensure settings expose only the background image basename, not its path.
    #[test]
    fn the_background_image_name_is_derived_from_the_stored_path() {
        let conn = open_in_memory(APP_MIGRATIONS).unwrap();
        seed_default_settings(&conn).unwrap();

        write_setting(&conn, THEME_BACKGROUND_KEY, "/pics/wall/ridge.png").unwrap();

        let settings = read_settings(&conn).unwrap();
        assert_eq!(settings.custom_background_image.as_deref(), Some("ridge.png"));

        // Writing the settings back must not clobber the stored path with the
        // name that came out of the read.
        persist_settings(&conn, &settings).unwrap();
        assert_eq!(
            read_setting(&conn, THEME_BACKGROUND_KEY).unwrap().unwrap(),
            "/pics/wall/ridge.png"
        );

        // Clearing removes the row outright rather than leaving a blank one.
        delete_setting(&conn, THEME_BACKGROUND_KEY).unwrap();
        assert_eq!(read_settings(&conn).unwrap().custom_background_image, None);
    }

    #[test]
    fn errors_are_categorised_for_the_ui() {
        let error = CommandError::from(LibraryError::NoOpenLibrary);
        assert_eq!(error.category, "no_library");

        let error = CommandError::from(LibraryError::AssetNotFound(9));
        assert_eq!(error.category, "not_found");

        let error = CommandError::from(LibraryError::Invalid("bad".into()));
        assert_eq!(error.category, "invalid");
    }
}