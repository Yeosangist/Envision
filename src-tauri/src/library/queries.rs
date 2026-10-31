
//! Read queries over the library index, plus the database-only reorder.
use super::{LibraryError, LibraryIndex, Result};
use crate::domain::{Asset, DuplicateGroup, ReorderResult};

/// Join per-hash counts so tiles can show duplicates without an N+1 query.
const DUPLICATE_JOIN: &str = "
    LEFT JOIN (
        SELECT sha256, COUNT(*) AS copies
        FROM assets
        WHERE sha256 IS NOT NULL
        GROUP BY sha256
    ) dupes ON dupes.sha256 = a.sha256";

fn asset_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        filename: row.get(2)?,
        rel_path: row.get(3)?,
        ext: row.get(4)?,
        width: row.get(5)?,
        height: row.get(6)?,
        decode_ok: row.get::<_, i64>(7)? != 0,
        position: row.get(8)?,
        duplicate_count: row.get::<_, Option<i64>>(9)?.unwrap_or(0).max(0),
        bytes: row.get(10)?,
        mtime: row.get(11)?,
        sha256: row.get(12)?,
        // Filled in by `attach_thumb_urls`, which needs the cache location.
        thumb_url: None,
    })
}

const SELECT_ASSET: &str = "
    SELECT a.id, a.folder_id, a.filename, a.rel_path, a.ext,
           a.width, a.height, a.decode_ok, a.position,
           COALESCE(dupes.copies, 1) - 1 AS duplicate_count,
           a.bytes, a.mtime, a.sha256
    FROM assets a";

/// Use SHA-256 keys for shared content, or a stable per-asset key until fingerprinting completes.
pub fn thumbnail_key(asset_id: i64, sha256: Option<&str>) -> String {
    match sha256 {
        Some(hash) => hash.to_string(),
        None => format!("pending-{asset_id:016x}"),
    }
}

/// Attach backend-generated thumbnail URLs to decodable assets; unsupported formats get `None`.
pub fn attach_thumb_urls(index: &LibraryIndex, assets: &mut [Asset]) {
    for asset in assets.iter_mut() {
        asset.thumb_url = if asset.decode_ok {
            let key = thumbnail_key(asset.id, asset.sha256.as_deref());
            crate::thumbnails::path_for(&index.thumbnails_dir, &key).map(|path| {
                crate::platform::thumbnail_url(&index.thumbnails_dir, &index.id, &path)
            })
        } else {
            None
        };
    }
}

/// List assets in `assets.position` order; reordering changes index state only.
pub fn list_assets(index: &LibraryIndex, folder_id: i64) -> Result<Vec<Asset>> {
    let mut stmt = index.conn.prepare(&format!(
        "{SELECT_ASSET} {DUPLICATE_JOIN}
         WHERE a.folder_id = ?1
         ORDER BY a.position ASC, a.id ASC"
    ))?;

    let mut assets = stmt
        .query_map([folder_id], asset_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    attach_thumb_urls(index, &mut assets);
    Ok(assets)
}

pub fn asset_by_id(index: &LibraryIndex, id: i64) -> Result<Asset> {
    index
        .conn
        .query_row(
            &format!("{SELECT_ASSET} {DUPLICATE_JOIN} WHERE a.id = ?1"),
            [id],
            asset_from_row,
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => LibraryError::AssetNotFound(id),
            other => other.into(),
        })
}

/// Look up an asset by its stored relative path.
#[cfg(test)]
pub(crate) fn asset_by_rel(index: &LibraryIndex, rel_path: &str) -> Result<Option<Asset>> {
    use rusqlite::OptionalExtension;

    let asset = index
        .conn
        .query_row(
            &format!("{SELECT_ASSET} {DUPLICATE_JOIN} WHERE a.rel_path = ?1"),
            [rel_path],
            asset_from_row,
        )
        .optional()?;
    Ok(asset)
}

/// Group identical SHA-256 assets for detection only; files remain present and browsable.
pub fn list_duplicates(index: &LibraryIndex) -> Result<Vec<DuplicateGroup>> {
    // First, get all duplicate groups
    let mut stmt = index.conn.prepare(
        "SELECT sha256, MIN(bytes) AS bytes, COUNT(*) AS copies
         FROM assets
         WHERE sha256 IS NOT NULL
         GROUP BY sha256
         HAVING COUNT(*) > 1
         ORDER BY copies DESC, MIN(filename) ASC",
    )?;

    let groups = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if groups.is_empty() {
        return Ok(Vec::new());
    }

    // Collect all sha256s for a single query
    let sha256s: Vec<String> = groups.iter().map(|(s, _, _)| s.clone()).collect();
    let placeholders: String = (0..sha256s.len()).map(|_| "?").collect::<Vec<_>>().join(",");

    let mut members_stmt = index.conn.prepare(&format!(
        "{SELECT_ASSET} {DUPLICATE_JOIN}
         WHERE a.sha256 IN ({})
         ORDER BY a.sha256, a.rel_path ASC",
        placeholders
    ))?;

    let mut params: Vec<&dyn rusqlite::ToSql> = Vec::with_capacity(sha256s.len());
    for s in &sha256s {
        params.push(s);
    }

    let mut all_assets = members_stmt
        .query_map(rusqlite::params_from_iter(params), asset_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    attach_thumb_urls(index, &mut all_assets);

    let mut assets_by_sha: std::collections::HashMap<String, Vec<Asset>> = std::collections::HashMap::new();
    for asset in all_assets {
        if let Some(ref sha) = asset.sha256 {
            assets_by_sha.entry(sha.clone()).or_default().push(asset);
        }
    }

    let mut out = Vec::with_capacity(groups.len());
    for (sha256, bytes, count) in groups {
        let assets = assets_by_sha.remove(&sha256).unwrap_or_default();
        out.push(DuplicateGroup {
            sha256,
            bytes,
            count,
            assets,
        });
    }

    Ok(out)
}

/// How many assets share bytes with something else in the library.
pub fn duplicate_summary(index: &LibraryIndex) -> Result<(i64, i64)> {
    let (groups, redundant) = index.conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(copies - 1), 0) FROM (
             SELECT COUNT(*) AS copies FROM assets
             WHERE sha256 IS NOT NULL
             GROUP BY sha256
             HAVING COUNT(*) > 1
         )",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok((groups, redundant))
}

/// The position a newly discovered image should take: after everything already
/// in the folder.
pub fn next_position(conn: &rusqlite::Connection, folder_id: i64) -> Result<i64> {
    let next = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM assets WHERE folder_id = ?1",
        [folder_id],
        |row| row.get(0),
    )?;
    Ok(next)
}

/// Update only `assets.position`; `ordered_ids` must be a complete permutation of the folder's assets.
pub fn reorder_assets(
    index: &LibraryIndex,
    folder_id: i64,
    ordered_ids: &[i64],
) -> Result<ReorderResult> {
    let current: Vec<i64> = {
        let mut stmt = index
            .conn
            .prepare("SELECT id FROM assets WHERE folder_id = ?1")?;
        let ids = stmt
            .query_map([folder_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        ids
    };

    let mut current = current;
    current.sort_unstable();

    let mut proposed = ordered_ids.to_vec();
    proposed.sort_unstable();
    proposed.dedup();

    if proposed != current {
        return Err(LibraryError::Invalid(format!(
            "reorder must list every image in the folder exactly once (expected {}, got {})",
            current.len(),
            proposed.len()
        )));
    }

    let tx = index.conn.unchecked_transaction()?;
    {
        let mut update = tx.prepare(
            "UPDATE assets SET position = ?1 WHERE id = ?2 AND folder_id = ?3",
        )?;
        for (position, asset_id) in ordered_ids.iter().enumerate() {
            update.execute(rusqlite::params![position as i64, asset_id, folder_id])?;
        }
    }
    tx.commit()?;

    let positions = ordered_ids
        .iter()
        .enumerate()
        .map(|(position, asset_id)| crate::domain::AssetPosition {
            asset_id: *asset_id,
            position: position as i64,
        })
        .collect();

    Ok(ReorderResult {
        folder_id,
        positions,
    })
}

/// Re-densify positions for a folder after its membership changed, preserving
/// the relative order that is already stored.
pub fn compact_positions(conn: &rusqlite::Connection, folder_id: i64) -> Result<()> {
    let ordered: Vec<(i64, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT id, position FROM assets WHERE folder_id = ?1 ORDER BY position ASC, id ASC",
        )?;
        let rows = stmt
            .query_map([folder_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };

    let tx = conn.unchecked_transaction()?;
    {
        let mut update = tx.prepare("UPDATE assets SET position = ?1 WHERE id = ?2")?;
        for (position, (id, _)) in ordered.iter().enumerate() {
            update.execute(rusqlite::params![position as i64, id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use crate::database::{open_in_memory, LIBRARY_MIGRATIONS};
    use std::path::PathBuf;

    /// An in-memory index rooted at a path that need not exist. Shared by the
    /// unit tests across the `library` modules.
    pub(crate) fn index() -> LibraryIndex {
        LibraryIndex {
            id: "test".into(),
            root: PathBuf::from("/library"),
            conn: open_in_memory(LIBRARY_MIGRATIONS).unwrap(),
            thumbnails_dir: PathBuf::from("/tmp/thumbs"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::index;
    use super::*;

    fn seed(index: &LibraryIndex, folder: &str, files: &[(&str, &str)]) {
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, '', '', 0)",
                [],
            )
            .unwrap();
        for (position, (name, sha)) in files.iter().enumerate() {
            let rel = if folder.is_empty() {
                name.to_string()
            } else {
                format!("{folder}/{name}")
            };
            index
                .conn
                .execute(
                    "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                         sha256, position, created_at, indexed_at)
                     VALUES (1, ?1, ?2, 'jpg', 100, 0, ?3, ?4, 0, 0)",
                    rusqlite::params![name, rel, sha, position as i64],
                )
                .unwrap();
        }
    }

    #[test]
    fn assets_are_listed_in_stored_order() {
        let index = index();
        seed(
            &index,
            "",
            &[("a.jpg", "aa"), ("b.jpg", "bb"), ("c.jpg", "cc")],
        );

        let assets = list_assets(&index, 1).unwrap();
        assert_eq!(
            assets.iter().map(|a| a.filename.as_str()).collect::<Vec<_>>(),
            vec!["a.jpg", "b.jpg", "c.jpg"]
        );
        assert!(assets.iter().all(|a| a.duplicate_count == 0));
    }

    #[test]
    fn reorder_rewrites_order_without_touching_paths() {
        let index = index();
        seed(
            &index,
            "album",
            &[("a.jpg", "aa"), ("b.jpg", "bb"), ("c.jpg", "cc")],
        );
        let before: Vec<(String, String)> = index
            .conn
            .prepare("SELECT filename, rel_path FROM assets ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        let ids: Vec<i64> = list_assets(&index, 1).unwrap().iter().map(|a| a.id).collect();
        let flipped = vec![ids[2], ids[0], ids[1]];
        reorder_assets(&index, 1, &flipped).unwrap();

        let after: Vec<(String, String)> = index
            .conn
            .prepare("SELECT filename, rel_path FROM assets ORDER BY id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(
            before, after,
            "reordering must not alter any stored path"
        );

        let ordered: Vec<String> = list_assets(&index, 1)
            .unwrap()
            .iter()
            .map(|a| a.filename.clone())
            .collect();
        assert_eq!(ordered, vec!["c.jpg", "a.jpg", "b.jpg"]);

        // And the rel_path is genuinely unrelated to display order.
        assert!(ordered
            .iter()
            .any(|name| format!("album/{name}") != before[0].1));
    }

    #[test]
    fn reorder_rejects_a_partial_or_duplicated_list() {
        let index = index();
        seed(&index, "", &[("a.jpg", "aa"), ("b.jpg", "bb"), ("c.jpg", "cc")]);
        let ids: Vec<i64> = list_assets(&index, 1).unwrap().iter().map(|a| a.id).collect();

        assert!(reorder_assets(&index, 1, &ids[..2]).is_err());
        assert!(reorder_assets(&index, 1, &[ids[0], ids[0], ids[1]]).is_err());
        assert!(reorder_assets(&index, 1, &[ids[0], ids[1], 9999]).is_err());

        // A rejected reorder leaves the order untouched.
        let order: Vec<i64> = list_assets(&index, 1)
            .unwrap()
            .iter()
            .map(|a| a.id)
            .collect();
        assert_eq!(order, ids);
    }

    #[test]
    fn duplicates_are_grouped_and_counted_without_hiding_members() {
        let index = index();
        seed(
            &index,
            "album",
            &[
                ("a.jpg", "aa"), // unique
                ("b.jpg", "bb"),
                ("c.jpg", "bb"), // duplicate of b, different name
                ("d.jpg", "bb"),
            ],
        );

        let groups = list_duplicates(&index).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].count, 3);
        assert_eq!(groups[0].assets.len(), 3);

        // Every member is still individually listed in the folder.
        let all = list_assets(&index, 1).unwrap();
        assert_eq!(all.len(), 4, "no asset is hidden by deduplication");
        assert!(all.iter().filter(|a| a.duplicate_count == 2).count() == 3);
        assert_eq!(all.iter().find(|a| a.filename == "a.jpg").unwrap().duplicate_count, 0);

        let (group_count, redundant) = duplicate_summary(&index).unwrap();
        assert_eq!((group_count, redundant), (1, 2));
    }

    #[test]
    fn unhashed_assets_are_never_grouped() {
        let index = index();
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, '', '', 0)",
                [],
            )
            .unwrap();
        for name in ["a.jpg", "b.jpg"] {
            index
                .conn
                .execute(
                    "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                         sha256, position, created_at, indexed_at)
                     VALUES (1, ?1, ?1, 'jpg', 100, 0, NULL, 0, 0, 0)",
                    [name],
                )
                .unwrap();
        }

        assert!(list_duplicates(&index).unwrap().is_empty());
        assert!(list_assets(&index, 1).unwrap().iter().all(|a| a.duplicate_count == 0));
    }

    #[test]
    fn compact_positions_preserves_relative_order() {
        let index = index();
        seed(&index, "", &[("a.jpg", "aa"), ("b.jpg", "bb"), ("c.jpg", "cc")]);
        index
            .conn
            .execute("UPDATE assets SET position = position * 10", [])
            .unwrap();

        compact_positions(&index.conn, 1).unwrap();

        let positions: Vec<i64> = index
            .conn
            .prepare("SELECT position FROM assets ORDER BY position")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(positions, vec![0, 1, 2]);
    }

    #[test]
    fn asset_lookup_by_rel_path_works_across_folders() {
        let index = index();
        seed(&index, "", &[("a.jpg", "aa")]);
        let found = asset_by_rel(&index, "a.jpg").unwrap().unwrap();
        assert_eq!(found.filename, "a.jpg");
        assert!(asset_by_rel(&index, "missing.jpg").unwrap().is_none());
        assert!(matches!(
            asset_by_id(&index, 4242),
            Err(LibraryError::AssetNotFound(4242))
        ));
    }


    // --- thumbnail URLs -----------------------------------------------------
    #[test]
    fn thumbnail_keys_fall_back_per_asset_until_fingerprinted() {
        assert_eq!(thumbnail_key(7, Some("abc123")), "abc123");
        // Stable across calls, distinct per asset, and cache-path safe.
        assert_eq!(thumbnail_key(7, None), thumbnail_key(7, None));
        assert_ne!(thumbnail_key(7, None), thumbnail_key(8, None));
        assert!(crate::thumbnails::path_for(std::path::Path::new("/c"), &thumbnail_key(7, None)).is_some());
    }

    #[test]
    fn every_decodable_asset_carries_a_thumbnail_url() {
        let index = index();
        // Fingerprint characters are chosen so a filename can never appear as a
        // substring of its own hash-derived cache path.
        seed(
            &index,
            "",
            &[("a.jpg", &"f".repeat(64)), ("b.png", &"e".repeat(64))],
        );

        let assets = list_assets(&index, 1).unwrap();
        assert_eq!(assets.len(), 2);
        for asset in &assets {
            let url = asset.thumb_url.as_deref().expect("decodable asset needs a url");

            // Points into the thumbnail cache, never at the user's own file.
            assert!(url.contains("thumbs"), "{url}");
            assert!(!url.contains("a.jpg") && !url.contains("b.png"), "{url}");

            if cfg!(debug_assertions) {
                // Debug builds address the Vite dev server, same-origin.
                assert!(url.starts_with(crate::platform::DEV_THUMB_PREFIX), "{url}");
                assert!(!url.contains("%2F"), "dev URLs are not encoded: {url}");
            } else {
                assert!(
                    url.starts_with("asset://localhost/")
                        || url.starts_with("http://asset.localhost/"),
                    "{url}"
                );
                assert_eq!(url.matches('/').count(), 3, "path fully encoded: {url}");
            }
        }
    }

    #[test]
    fn a_fingerprint_that_cannot_key_a_path_yields_no_url() {
        let index = index();
        // A path separator in the key would let a corrupted row aim the cache
        // outside its own directory, so it is refused rather than escaped.
        seed(&index, "", &[("a.jpg", "aa/aa")]);

        let assets = list_assets(&index, 1).unwrap();
        assert!(assets[0].sha256.is_some());
        assert!(
            assets[0].thumb_url.is_none(),
            "a key that cannot address the cache must not produce a URL"
        );
    }

    #[test]
    fn formats_envision_cannot_decode_get_no_thumbnail_url() {
        let index = index();
        index
            .conn
            .execute(
                "INSERT INTO folders (id, parent_id, name, rel_path, created_at)
                 VALUES (1, NULL, '', '', 0)",
                [],
            )
            .unwrap();
        index
            .conn
            .execute(
                "INSERT INTO assets (folder_id, filename, rel_path, ext, bytes, mtime,
                                     decode_ok, position, created_at, indexed_at)
                 VALUES (1, 'p.heic', 'p.heic', 'heic', 9, 0, 0, 0, 0, 0)",
                [],
            )
            .unwrap();

        let assets = list_assets(&index, 1).unwrap();
        assert_eq!(assets.len(), 1);
        assert!(!assets[0].decode_ok);
        assert!(
            assets[0].thumb_url.is_none(),
            "an undecodable file must not advertise a thumbnail"
        );
    }

    #[test]
    fn identical_bytes_share_one_thumbnail_url() {
        let index = index();
        let hash = "d".repeat(64);
        seed(&index, "", &[("a.jpg", &hash), ("b.jpg", &hash)]);

        let assets = list_assets(&index, 1).unwrap();
        assert_eq!(assets[0].thumb_url, assets[1].thumb_url);
    }
}