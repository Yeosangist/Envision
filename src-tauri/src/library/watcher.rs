
use notify::{EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const DEBOUNCE: Duration = Duration::from_millis(700);
const MAX_DEFER: Duration = Duration::from_secs(5);

pub const CHANGED_EVENT: &str = "library:changed";

pub struct WatcherHandle {
    _watcher: notify::RecommendedWatcher,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for WatcherHandle {
    fn drop(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = thread;
        }
    }
}

pub fn start(app: AppHandle, library_id: String, root: PathBuf) -> notify::Result<WatcherHandle> {
    let (tx, rx) = channel::<notify::Result<notify::Event>>();

    let mut watcher = notify::recommended_watcher(move |event| {
        let _ = tx.send(event);
    })?;

    watcher.watch(&root, RecursiveMode::Recursive)?;

    let thread = std::thread::Builder::new()
        .name(format!("envision-watch-{library_id}"))
        .spawn(move || debounce_loop(app, library_id, rx))?;

    Ok(WatcherHandle {
        _watcher: watcher,
        thread: Some(thread),
    })
}

fn debounce_loop(
    app: AppHandle,
    library_id: String,
    rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
) {
    let mut first_pending: Option<Instant> = None;
    let mut interesting: Vec<String> = Vec::new();

    loop {
        match rx.recv_timeout(DEBOUNCE) {
            Ok(Ok(event)) => {
                if !is_interesting(&event) {
                    continue;
                }
                for path in &event.paths {
                    if let Some(label) = describe(path) {
                        if !interesting.contains(&label) {
                            interesting.push(label);
                        }
                    }
                }
                first_pending.get_or_insert_with(Instant::now);
            }
            Ok(Err(_)) => continue,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        let Some(since) = first_pending else {
            continue;
        };
        if since.elapsed() < DEBOUNCE && since.elapsed() < MAX_DEFER {
            continue;
        }
        first_pending = None;

        let payload = serde_json::json!({
            "library_id": library_id,
            "paths": interesting,
        });
        let _ = app.emit(CHANGED_EVENT, payload);
        interesting.clear();
    }
}

fn is_interesting(event: &notify::Event) -> bool {
    match event.kind {
        EventKind::Access(_) => false,
        _ => true,
    }
}

fn describe(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().to_string();
    if name.ends_with("~") || name.starts_with(".goutputstream") || name.ends_with(".tmp") {
        return None;
    }
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_events_are_not_worth_a_rescan() {
        let mut event = notify::Event::new(notify::EventKind::Access(notify::event::AccessKind::Read));
        event.paths = vec![PathBuf::from("/library/a.png")];
        assert!(!is_interesting(&event));
    }

    #[test]
    fn content_changes_are_worth_a_rescan() {
        let mut event =
            notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Any));
        event.paths = vec![PathBuf::from("/library/a.png")];
        assert!(is_interesting(&event));

        let mut created =
            notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File));
        created.paths = vec![PathBuf::from("/library/b.png")];
        assert!(is_interesting(&created));
    }

    #[test]
    fn temporary_and_editor_scratch_files_are_ignored() {
        assert!(describe(Path::new("/library/a.png~")).is_none());
        assert!(describe(Path::new("/library/.goutputstream-x")).is_none());
        assert!(describe(Path::new("/library/partial.tmp")).is_none());
        assert_eq!(
            describe(Path::new("/library/holiday.png")),
            Some("holiday.png".to_string())
        );
        assert!(describe(Path::new("/")).is_none());
    }
}