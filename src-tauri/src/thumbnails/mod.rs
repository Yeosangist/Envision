
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ThumbnailError {
    #[error("Image processing error: {0}")]
    Processing(#[from] image::ImageError),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, ThumbnailError>;

pub fn worker_count_for(limit: i64, available: usize) -> usize {
    let available = available.max(1);
    if limit <= 0 {
        available
    } else {
        (limit as usize).min(available).max(1)
    }
}

pub fn worker_count(limit: i64) -> usize {
    worker_count_for(
        limit,
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1),
    )
}

struct WorkerPool {
    threads: usize,
    pool: Arc<rayon::ThreadPool>,
}

fn shared_pool() -> &'static Mutex<Option<WorkerPool>> {
    static POOL: OnceLock<Mutex<Option<WorkerPool>>> = OnceLock::new();
    POOL.get_or_init(|| Mutex::new(None))
}

fn pool_for(threads: usize) -> Option<Arc<rayon::ThreadPool>> {
    let mut current = shared_pool().lock().ok()?;
    if let Some(existing) = current.as_ref() {
        if existing.threads == threads {
            return Some(existing.pool.clone());
        }
    }

    match rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|index| format!("thumbnail-{index}"))
        .build()
    {
        Ok(pool) => {
            let pool = Arc::new(pool);
            *current = Some(WorkerPool {
                threads,
                pool: pool.clone(),
            });
            log::info!("thumbnail pool set to {threads} worker(s)");
            Some(pool)
        }
        Err(error) => {
            log::warn!("could not build the thumbnail thread pool: {error}");
            None
        }
    }
}

pub fn with_worker_pool<T, F>(limit: i64, work: F) -> T
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    match pool_for(worker_count(limit)) {
        Some(pool) => pool.install(work),
        None => work(),
    }
}

pub const CANONICAL_WIDTH: u32 = 384;

const QUALITY: u8 = 75;

pub const CACHE_VERSION: u32 = 3;

fn versioned_dir(cache_dir: &Path) -> PathBuf {
    cache_dir.join(format!("v{CACHE_VERSION}"))
}

pub fn path_for(cache_dir: &Path, key: &str) -> Option<PathBuf> {
    if key.len() < 4 || key.contains('/') || key.contains('\\') || key.contains('\0') {
        return None;
    }
    if !key
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    Some(versioned_dir(cache_dir).join(&key[..2]).join(format!("{key}.jpg")))
}

pub fn is_cached(cache_dir: &Path, sha256: &str) -> bool {
    path_for(cache_dir, sha256).is_some_and(|path| {
        path.metadata().map(|meta| meta.len() > 0).unwrap_or(false)
    })
}

pub fn ensure(
    cache_dir: &Path,
    sha256: &str,
    source: &Path,
    decode_ok: bool,
) -> Result<Option<(PathBuf, u32, u32)>> {
    if !decode_ok {
        return Ok(None);
    }
    let Some(destination) = path_for(cache_dir, sha256) else {
        return Ok(None);
    };

    if is_cached(cache_dir, sha256) {
        if let Some((width, height)) = dimensions_of(&destination) {
            return Ok(Some((destination, width, height)));
        }
    }

    let decode_start = std::time::Instant::now();
    let image = image::load_from_memory(&std::fs::read(source)?)?;
    let decode_time = decode_start.elapsed();
    if decode_time.as_secs() > 5 {
        log::warn!("Slow decode: {decode_time:?} for {source:?}");
    }

    let resize_start = std::time::Instant::now();
    let thumbnail = if image.width() > CANONICAL_WIDTH || image.height() > CANONICAL_WIDTH {
        image.thumbnail(CANONICAL_WIDTH, CANONICAL_WIDTH)
    } else {
        image
    };
    let resize_time = resize_start.elapsed();
    if resize_time.as_secs() > 5 {
        log::warn!("Slow resize: {resize_time:?} for {source:?}");
    }

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let staging = destination.with_extension("staging.jpg");
    let encode_start = std::time::Instant::now();
    let encoded: std::io::Result<()> = (|| {
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
            std::fs::File::create(&staging)?,
            QUALITY,
        );
        encoder
            .encode_image(&thumbnail)
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        drop(encoder);
        std::fs::rename(&staging, &destination)
    })();
    let encode_time = encode_start.elapsed();
    if encode_time.as_secs() > 5 {
        log::warn!("Slow encode: {encode_time:?} for {source:?}");
    }

    if let Err(error) = encoded {
        let _ = std::fs::remove_file(&staging);
        return Err(ThumbnailError::Io(error));
    }

    let (width, height) = dimensions_of(&destination).unwrap_or((0, 0));
    log::info!("Thumbnail timing: decode={decode_time:?} resize={resize_time:?} encode={encode_time:?} for {source:?}");
    Ok(Some((destination, width, height)))
}

fn dimensions_of(path: &Path) -> Option<(u32, u32)> {
    image::image_dimensions(path).ok()
}

pub fn prune(cache_dir: &Path, live_hashes: &[String]) -> Result<u64> {
    let mut removed = 0;

    if let Ok(entries) = std::fs::read_dir(cache_dir) {
        for entry in entries.filter_map(|entry| entry.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('v') && name != format!("v{CACHE_VERSION}") {
                if entry.path().is_dir() {
                    std::fs::remove_dir_all(entry.path()).ok();
                } else {
                    std::fs::remove_file(entry.path()).ok();
                }
                removed += 1;
            }
        }
    }

    let root = versioned_dir(cache_dir);
    let Ok(shards) = std::fs::read_dir(&root) else {
        return Ok(removed);
    };

    for shard in shards.filter_map(|entry| entry.ok()) {
        let shard_path = shard.path();
        let Ok(files) = std::fs::read_dir(&shard_path) else {
            continue;
        };

        for file in files.filter_map(|entry| entry.ok()) {
            let name = file.file_name().to_string_lossy().to_string();
            let Some(hash) = name.strip_suffix(".jpg") else {
                continue;
            };
            if !live_hashes.iter().any(|live| live == hash) {
                std::fs::remove_file(file.path()).ok();
                removed += 1;
            }
        }

        if std::fs::read_dir(&shard_path).is_ok_and(|mut d| d.next().is_none()) {
            std::fs::remove_dir(&shard_path).ok();
        }
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("envision-thumb-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const HASH: &str = "aabbccddeeff00112233445566778899aabbccddeeff00112233445566778899";

    #[test]
    fn worker_counts_handle_auto_limits_and_more_cores_than_exist() {
        assert_eq!(worker_count_for(0, 8), 8, "0 means every available core");
        assert_eq!(worker_count_for(-4, 8), 8, "negative values mean auto too");
        assert_eq!(worker_count_for(2, 8), 2);
        assert_eq!(
            worker_count_for(99, 8),
            8,
            "never spawn more than there are cores"
        );
        assert_eq!(worker_count_for(2, 0), 1, "always at least one worker");
    }

    #[test]
    fn a_limit_is_a_shared_ceiling_across_concurrent_batches() {
        use rayon::prelude::*;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        const LIMIT: i64 = 2;
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let batches: Vec<_> = (0..4)
            .map(|_| {
                let active = active.clone();
                let peak = peak.clone();
                std::thread::spawn(move || {
                    with_worker_pool(LIMIT, || {
                        (0..20).into_par_iter().for_each(|_| {
                            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            std::thread::sleep(std::time::Duration::from_millis(2));
                            active.fetch_sub(1, Ordering::SeqCst);
                        });
                    });
                })
            })
            .collect();

        for batch in batches {
            batch.join().unwrap();
        }

        let peak = peak.load(Ordering::SeqCst);
        assert!(
            peak <= LIMIT as usize,
            "{peak} thumbnail tasks ran at once with a limit of {LIMIT}"
        );

        assert_eq!(
            with_worker_pool(1, rayon::current_num_threads),
            1,
            "thumbnail work must run on the limited pool, not the global one"
        );
        assert_eq!(
            with_worker_pool(LIMIT, rayon::current_num_threads),
            LIMIT as usize,
            "changing the limit must resize the shared pool"
        );
    }

    #[test]
    fn cache_paths_are_sharded_and_versioned() {
        let dir = Path::new("/cache");
        let path = path_for(dir, HASH).unwrap();

        assert_eq!(
            path.parent().unwrap(),
            Path::new("/cache").join(format!("v{CACHE_VERSION}")).join("aa")
        );
        assert_eq!(
            path.file_name().unwrap().to_string_lossy(),
            format!("{HASH}.jpg")
        );
    }

    #[test]
    fn malformed_fingerprints_have_no_cache_path() {
        for bad in ["", "aa", "not hex at all!!", "../escape", "a/b"] {
            assert!(path_for(Path::new("/cache"), bad).is_none(), "for {bad:?}");
        }
    }

    #[test]
    fn pending_keys_get_a_cache_path_too() {
        let key = "pending-0000000000000042";
        assert!(path_for(Path::new("/cache"), key).is_some());
    }

    #[test]
    fn generates_then_reuses_a_thumbnail() {
        let cache = temp_dir("generate");
        let source = temp_dir("source");
        let image_path = source.join("wide.png");

        image::RgbImage::from_raw(800, 400, vec![128u8; 800 * 400 * 3])
            .unwrap()
            .save(&image_path)
            .unwrap();

        let (path, width, height) = ensure(&cache, HASH, &image_path, true)
            .unwrap()
            .unwrap();

        assert!(path.is_file());
        assert_eq!(width, CANONICAL_WIDTH);
        assert_eq!(height, 192, "aspect ratio is preserved");
        assert!(is_cached(&cache, HASH));

        let (again, _, _) = ensure(&cache, HASH, &image_path, true).unwrap().unwrap();
        assert_eq!(again, path);

        std::fs::remove_dir_all(&cache).ok();
        std::fs::remove_dir_all(&source).ok();
    }

    #[test]
    fn generates_a_thumbnail_for_a_jpeg_that_claims_to_be_a_png() {
        let cache = temp_dir("mislabelled");
        let source = temp_dir("mislabelled-source");
        let image_path = source.join("liar.png");

        let other = "b".repeat(64);
        image::DynamicImage::ImageRgb8(
            image::RgbImage::from_raw(900, 600, vec![90u8; 900 * 600 * 3]).unwrap(),
        )
        .save_with_format(&image_path, image::ImageFormat::Jpeg)
        .unwrap();

        let (path, width, height) = ensure(&cache, &other, &image_path, true)
            .unwrap()
            .unwrap_or_else(|| panic!("thumbnail must be produced for {image_path:?}"));

        assert!(path.is_file());
        assert_eq!(width, CANONICAL_WIDTH);
        assert_eq!(height, 256, "aspect ratio comes from the real pixels");
        assert!(is_cached(&cache, &other));

        std::fs::remove_dir_all(&cache).ok();
        std::fs::remove_dir_all(&source).ok();
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let cache = temp_dir("small");
        let source = temp_dir("small-source");
        let image_path = source.join("tiny.png");

        image::RgbImage::from_raw(64, 48, vec![10u8; 64 * 48 * 3])
            .unwrap()
            .save(&image_path)
            .unwrap();

        let (_, width, height) = ensure(&cache, HASH, &image_path, true).unwrap().unwrap();
        assert_eq!((width, height), (64, 48));

        std::fs::remove_dir_all(&cache).ok();
        std::fs::remove_dir_all(&source).ok();
    }

    #[test]
    fn large_images_are_reduced_to_the_canonical_width() {
        let cache = temp_dir("large");
        let source = temp_dir("large-source");
        let image_path = source.join("huge.jpg");

        image::RgbImage::from_raw(3000, 2000, vec![20u8; 3000 * 2000 * 3])
            .unwrap()
            .save(&image_path)
            .unwrap();

        let (_, width, height) = ensure(&cache, HASH, &image_path, true).unwrap().unwrap();
        assert_eq!(width, CANONICAL_WIDTH);
        assert_eq!(height, 256, "2000 * 384/3000 rounds to 256");

        std::fs::remove_dir_all(&cache).ok();
        std::fs::remove_dir_all(&source).ok();
    }

    #[test]
    fn undecodable_formats_get_no_thumbnail() {
        let cache = temp_dir("external");
        let source = temp_dir("external-source");
        let image_path = source.join("photo.heic");
        std::fs::write(&image_path, b"ftypheic").unwrap();

        assert!(ensure(&cache, HASH, &image_path, false).unwrap().is_none());
        assert!(!is_cached(&cache, HASH));

        std::fs::remove_dir_all(&cache).ok();
        std::fs::remove_dir_all(&source).ok();
    }

    #[test]
    fn no_partial_file_survives_a_successful_write() {
        let cache = temp_dir("atomic");
        let source = temp_dir("staging-source");
        let image_path = source.join("a.png");
        image::RgbImage::from_raw(400, 400, vec![7u8; 400 * 400 * 3])
            .unwrap()
            .save(&image_path)
            .unwrap();

        ensure(&cache, HASH, &image_path, true).unwrap().unwrap();

        let leftovers: Vec<_> = walkdir::WalkDir::new(&cache)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.contains("staging") || name.ends_with(".partial")
            })
            .map(|e| e.path().display().to_string())
            .collect();
        assert!(leftovers.is_empty(), "left behind: {leftovers:?}");

        std::fs::remove_dir_all(&cache).ok();
        std::fs::remove_dir_all(&source).ok();
    }

    #[test]
    fn prune_removes_only_unreferenced_entries() {
        let cache = temp_dir("prune");
        let live = "11".to_string() + &"0".repeat(62);
        let dead = "22".to_string() + &"0".repeat(62);

        let live_path = path_for(&cache, &live).unwrap();
        let dead_path = path_for(&cache, &dead).unwrap();
        std::fs::create_dir_all(live_path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(dead_path.parent().unwrap()).unwrap();
        std::fs::write(&live_path, b"x").unwrap();
        std::fs::write(&dead_path, b"x").unwrap();

        let removed = prune(&cache, &[live.clone()]).unwrap();

        assert_eq!(removed, 1);
        assert!(live_path.exists());
        assert!(!dead_path.exists(), "empty shard is reclaimed");
    }

    #[test]
    fn prune_discards_superseded_cache_versions() {
        let cache = temp_dir("versions");

        let live = "33".to_string() + &"0".repeat(62);
        let current = path_for(&cache, &live).unwrap();
        std::fs::create_dir_all(current.parent().unwrap()).unwrap();
        std::fs::write(&current, b"x").unwrap();

        let stale_dir = cache.join("v1").join(&live[..2]);
        std::fs::create_dir_all(&stale_dir).unwrap();
        std::fs::write(stale_dir.join(format!("{live}.jpg")), b"x").unwrap();

        let removed = prune(&cache, &[live.clone()]).unwrap();

        assert!(!cache.join("v1").exists(), "stale version removed wholesale");
        assert!(current.exists(), "current version survives");
        assert_eq!(removed, 1);
    }
}