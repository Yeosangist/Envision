
//! Hash complete file bytes with SHA-256; size and mtime only decide when to re-read.
use super::{LibraryIndex, Result};
use sha2::{Digest, Sha256};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const READ_BUFFER: usize = 128 * 1024;
/// Rows buffered before flushing a batch write.
const WRITE_BATCH: usize = 64;

/// Stream a file through a fixed buffer and return its complete SHA-256.
pub fn fingerprint_file(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::with_capacity(READ_BUFFER, file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; READ_BUFFER];

    loop {
        let read = std::io::Read::read(&mut reader, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    Ok(hex::encode(hasher.finalize()))
}

/// Handle used to stop an in-flight fingerprint run.
#[derive(Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// One file waiting to be fingerprinted.
pub struct Pending {
    pub asset_id: i64,
    pub path: PathBuf,
}

/// Fingerprint pending files on blocking worker threads, reporting progress and checking cancellation between files.
/// Files that disappear during the run are skipped rather than failing the batch.
pub fn fingerprint_all<P>(
    index: &LibraryIndex,
    pending: Vec<Pending>,
    concurrency: usize,
    cancel: &Cancel,
    mut on_progress: P,
) -> Result<FingerprintOutcome>
where
    P: FnMut(u64, u64, &Path),
{
    let total = pending.len() as u64;
    let started = Instant::now();

    if pending.is_empty() {
        return Ok(FingerprintOutcome {
            fingerprinted: 0,
            missing: 0,
            elapsed: started.elapsed(),
        });
    }

    let workers = std::cmp::max(1, std::cmp::min(concurrency, pending.len()));
    let queue = Arc::new(Mutex::new(pending.into_iter().collect::<VecDeque<_>>()));
    let (tx, rx) = std::sync::mpsc::channel::<(Pending, Option<String>)>();

    let mut handles = Vec::with_capacity(workers);
    for _ in 0..workers {
        let queue = Arc::clone(&queue);
        let tx = tx.clone();
        let cancel = cancel.clone();
        handles.push(std::thread::spawn(move || {
            loop {
                // Checked between files so a stop takes effect promptly without
                // ever interrupting a read partway through.
                if cancel.is_cancelled() {
                    break;
                }
                let job = { queue.lock().expect("fingerprint queue poisoned").pop_front() };
                let Some(job) = job else { break };
                let hash = fingerprint_file(&job.path).ok();
                if tx.send((job, hash)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    let mut fingerprinted = 0u64;
    let mut missing = 0u64;
    let mut completed = 0u64;
    let mut batch: Vec<(i64, String)> = Vec::with_capacity(WRITE_BATCH);

    // The channel closes only once every worker has finished, so this loop ends
    // when the run genuinely is over rather than when the queue empties.
    while let Ok((job, hash)) = rx.recv() {
        match hash {
            Some(hash) => {
                batch.push((job.asset_id, hash));
                fingerprinted += 1;
            }
            None => missing += 1,
        }

        completed += 1;
        on_progress(completed, total, &job.path);

        if batch.len() >= WRITE_BATCH && !cancel.is_cancelled() {
            flush(index, &mut batch)?;
        }
    }

    for handle in handles {
        let _ = handle.join();
    }
    flush(index, &mut batch)?;

    Ok(FingerprintOutcome {
        fingerprinted,
        missing,
        elapsed: started.elapsed(),
    })
}

fn flush(index: &LibraryIndex, batch: &mut Vec<(i64, String)>) -> Result<()> {
    if batch.is_empty() {
        return Ok(());
    }

    let tx = index.conn.unchecked_transaction()?;
    {
        let mut update = tx.prepare(
            "UPDATE assets SET sha256 = ?1, hashed_at = ?2 WHERE id = ?3",
        )?;
        let now = chrono::Utc::now().timestamp();
        for (asset_id, hash) in batch.iter() {
            update.execute(rusqlite::params![hash, now, asset_id])?;
        }
    }
    tx.commit()?;
    batch.clear();
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub struct FingerprintOutcome {
    pub fingerprinted: u64,
    pub missing: u64,
    pub elapsed: std::time::Duration,
}

/// A row as the scanner last recorded it, used to decide whether a file needs
/// re-reading.
struct FingerprintState {
    id: i64,
    rel_path: String,
    bytes: i64,
    mtime: i64,
    sha256: Option<String>,
}

/// Queue unhashed assets and those whose size or mtime changed; SHA-256 of read bytes remains the identity.
pub fn collect_pending(index: &LibraryIndex) -> Result<Vec<Pending>> {
    let states: Vec<FingerprintState> = {
        let mut stmt = index.conn.prepare(
            "SELECT id, rel_path, bytes, mtime, sha256 FROM assets
             ORDER BY folder_id, position, id",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(FingerprintState {
                    id: row.get(0)?,
                    rel_path: row.get(1)?,
                    bytes: row.get(2)?,
                    mtime: row.get(3)?,
                    sha256: row.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let mut pending = Vec::new();
    let mut refreshed: Vec<(i64, i64)> = Vec::new();

    for state in states {
        // The stored rel_path is untrusted input; re-derive and validate it.
        let Some(path) = index.asset_path(&state.rel_path)? else {
            continue;
        };
        let Ok(metadata) = std::fs::metadata(&path) else {
            // Gone; the scan pass is responsible for removing the row.
            continue;
        };

        let bytes = metadata.len() as i64;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);

        if bytes != state.bytes || mtime != state.mtime {
            refreshed.push((state.id, bytes));
            index.conn.execute(
                "UPDATE assets SET bytes = ?1, mtime = ?2 WHERE id = ?3",
                rusqlite::params![bytes, mtime, state.id],
            )?;
        }

        let unchanged = state.sha256.is_some() && bytes == state.bytes && mtime == state.mtime;
        if !unchanged {
            pending.push(Pending {
                asset_id: state.id,
                path,
            });
        }
    }

    let _ = refreshed;
    Ok(pending)
}

/// Hashing bytes in isolation, for tests.
#[cfg(test)]
pub fn fingerprint_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str, contents: &[u8]) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("envision-hash-{tag}-{nanos}"));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn matches_the_published_sha256_vectors() {
        assert_eq!(
            fingerprint_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            fingerprint_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hashing_a_file_agrees_with_hashing_its_bytes() {
        let path = temp_file("agree", b"the quick brown fox");
        let on_disk = fingerprint_file(&path).unwrap();
        assert_eq!(on_disk, fingerprint_bytes(b"the quick brown fox"));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn identical_content_agrees_across_different_names() {
        let a = temp_file("dup-a", b"same bytes");
        let b = temp_file("dup-b", b"same bytes");
        assert_eq!(fingerprint_file(&a).unwrap(), fingerprint_file(&b).unwrap());
        std::fs::remove_file(&a).ok();
        std::fs::remove_file(&b).ok();
    }

    #[test]
    fn streams_files_larger_than_the_read_buffer() {
        let big = vec![0xABu8; READ_BUFFER * 3 + 17];
        let path = temp_file("big", &big);
        assert_eq!(fingerprint_file(&path).unwrap(), fingerprint_bytes(&big));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_missing_file_yields_no_hash_rather_than_panicking() {
        assert!(fingerprint_file(Path::new("/nonexistent/envision/nope.jpg")).is_err());
    }

    #[test]
    fn fingerprint_all_records_hashes_and_reports_progress() {
        let index = crate::library::queries::tests_support::index();
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, '', '', 0)",
                [],
            )
            .unwrap();

        let paths: Vec<PathBuf> = (0..5).map(|i| temp_file(&format!("batch{i}"), b"payload")).collect();
        for (position, path) in paths.iter().enumerate() {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            index
                .conn
                .execute(
                    "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                         position, created_at, indexed_at)
                     VALUES (1, ?1, ?1, 'jpg', 7, 0, ?2, 0, 0)",
                    rusqlite::params![name, position as i64],
                )
                .unwrap();
        }

        let pending: Vec<Pending> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| Pending {
                asset_id: i as i64 + 1,
                path: path.clone(),
            })
            .collect();

        let mut seen = Vec::new();
        let outcome = fingerprint_all(&index, pending, 4, &Cancel::new(), |done, total, _| {
            seen.push((done, total))
        })
        .unwrap();

        assert_eq!(outcome.fingerprinted, 5);
        assert_eq!(outcome.missing, 0);
        assert_eq!(seen.len(), 5);
        assert_eq!(seen.last().unwrap().0, 5);
        assert_eq!(seen.last().unwrap().1, 5);

        let stored: i64 = index
            .conn
            .query_row("SELECT COUNT(*) FROM assets WHERE sha256 IS NOT NULL", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(stored, 5);

        for path in &paths {
            std::fs::remove_file(path).ok();
        }
    }

    #[test]
    fn fingerprint_all_stops_when_cancelled() {
        let index = crate::library::queries::tests_support::index();
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, '', '', 0)",
                [],
            )
            .unwrap();

        let paths: Vec<PathBuf> = (0..8).map(|i| temp_file(&format!("cancel{i}"), b"x")).collect();
        for path in &paths {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            index
                .conn
                .execute(
                    "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                         position, created_at, indexed_at)
                     VALUES (1, ?1, ?1, 'jpg', 1, 0, 0, 0, 0)",
                    [name],
                )
                .unwrap();
        }

        let cancel = Cancel::new();
        cancel.cancel();

        let pending: Vec<Pending> = paths
            .iter()
            .enumerate()
            .map(|(i, path)| Pending {
                asset_id: i as i64 + 1,
                path: path.clone(),
            })
            .collect();

        let outcome = fingerprint_all(&index, pending, 2, &cancel, |_, _, _| {}).unwrap();
        assert_eq!(outcome.fingerprinted, 0, "a cancelled run must do no work");

        for path in &paths {
            std::fs::remove_file(path).ok();
        }
    }

    #[test]
    fn a_file_deleted_mid_run_is_skipped_not_fatal() {
        let index = crate::library::queries::tests_support::index();
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, '', '', 0)",
                [],
            )
            .unwrap();

        let alive = temp_file("survivor", b"still here");
        let gone = std::env::temp_dir().join("envision-hash-vanished-during-run");

        for name in [alive.file_name().unwrap(), gone.file_name().unwrap()] {
            index
                .conn
                .execute(
                    "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                         position, created_at, indexed_at)
                     VALUES (1, ?1, ?1, 'jpg', 1, 0, 0, 0, 0)",
                    [name.to_string_lossy().as_ref()],
                )
                .unwrap();
        }

        let pending = vec![
            Pending {
                asset_id: 1,
                path: alive.clone(),
            },
            Pending {
                asset_id: 2,
                path: gone,
            },
        ];

        let outcome = fingerprint_all(&index, pending, 2, &Cancel::new(), |_, _, _| {}).unwrap();
        assert_eq!(outcome.fingerprinted, 1);
        assert_eq!(outcome.missing, 1);

        std::fs::remove_file(&alive).ok();
    }
}