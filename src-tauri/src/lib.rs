mod commands;
mod database;
mod domain;
mod images;
mod library;
mod paths;
mod platform;
mod stats;
mod thumbnails;

use library::AppState;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Register first so startup and asset-protocol failures are captured.
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(2 * 1024 * 1024)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let app_db = commands::open_app_db(&handle)?;
            app.manage(Arc::new(AppState::new(handle, app_db)));
            if let (Some(icon), Some(window)) =
                (app.default_window_icon(), app.get_webview_window("main"))
            {
                window.set_icon(icon.clone())?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // libraries
            commands::add_library,
            commands::list_libraries,
            commands::open_library,
            commands::close_library,
            commands::forget_library,
            commands::relink_library,
            // scanning
            commands::scan_library,
            commands::cancel_scan,
            commands::scan_status,
            // folders and assets
            commands::list_child_folders,
            commands::list_folders,
            commands::folder_ancestry,
            commands::list_assets,
            commands::reorder_assets,
            commands::move_assets,
            commands::create_folder,
            commands::rename_folder,
            commands::read_asset_bytes,
            commands::open_asset_externally,
            // duplicates
            commands::list_duplicates,
            commands::duplicate_summary,
            commands::prune_thumbnails,
            commands::prune_all_thumbnails,
            // settings
            commands::get_settings,
            commands::update_settings,
            commands::set_theme_background,
            commands::check_gallery_dl,
            commands::generate_thumbnails_for_folder_cmd,
            commands::generate_thumbnails_for_assets_cmd,
            commands::read_theme_background,
            // statistics
            commands::library_stats,
            commands::library_summaries,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Envision");
}
