
//! Classify image formats, including those left for the desktop's external viewer.
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// The bundled `image` build can decode this; Envision renders it itself.
    Decodable,
    /// A still image format Envision indexes but does not decode. Rendered as a
    /// placeholder tile and opened via the desktop's external handler.
    External,
}

/// Extensions the bundled `image` crate decodes with default features.
const DECODABLE: &[&str] = &[
    "jpg", "jpeg", "jfif", "png", "gif", "webp", "bmp", "ico", "tiff", "tif", "tga", "qoi",
    "pnm", "pbm", "pam", "ppm", "pgm", "exr", "hdr", "dds", "pcx", "dcx", "farbfeld", "ff",
];

/// Still-image formats that are deliberately *not* decoded. These keep the
/// release free of native codec dependencies.
const EXTERNAL: &[&str] = &[
    "heic", "heif", "heics", "heifs", "avif", "avifs", "jxl", "svg", "svgz", "cr2", "cr3", "nef",
    "arw", "dng", "raf", "orf", "rw2", "pef", "srw", "raw",
];

/// Extension of `path`, lowercased, without the dot.
pub fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Classify a file by extension. `None` means "not an image at all" and the
/// file is ignored entirely by the scanner.
pub fn classify(ext: &str) -> Option<MediaKind> {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    if DECODABLE.contains(&ext.as_str()) {
        Some(MediaKind::Decodable)
    } else if EXTERNAL.contains(&ext.as_str()) {
        Some(MediaKind::External)
    } else {
        None
    }
}

pub fn is_indexable(path: &Path) -> bool {
    classify(&extension_of(path)).is_some()
}

pub fn mime_for(ext: &str) -> &'static str {
    match ext.trim_start_matches('.').to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "tiff" | "tif" => "image/tiff",
        "tga" | "pcx" | "dcx" => "image/x-tga",
        "qoi" => "image/qoi",
        "pnm" | "pbm" | "pam" | "ppm" | "pgm" => "image/x-portable-anymap",
        "exr" => "image/x-exr",
        "hdr" => "image/x-radiance",
        "dds" => "image/vnd-ms.dds",
        "farbfeld" | "ff" => "image/x-farbfeld",
        "heic" | "heics" => "image/heic",
        "heif" | "heifs" => "image/heif",
        "avif" | "avifs" => "image/avif",
        "jxl" => "image/jxl",
        "svg" | "svgz" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

/// Sniff image contents before choosing a decoder; retain extension fallback when sniffing fails.
pub fn open_by_content(path: &Path) -> image::ImageResult<image::DynamicImage> {
    image::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()
}

/// Read dimensions from the header without decoding; return `None` if detection or parsing fails.
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    image::ImageReader::open(path)
        .ok()?
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn classifies_common_formats_as_decodable() {
        for ext in ["jpg", "JPEG", ".png", "gif", "webp", "tiff"] {
            assert_eq!(classify(ext), Some(MediaKind::Decodable), "for {ext:?}");
        }
    }

    #[test]
    fn classifies_heic_and_friends_as_external() {
        for ext in ["heic", "HEIF", "avif", "jxl", "svg", "cr2"] {
            assert_eq!(classify(ext), Some(MediaKind::External), "for {ext:?}");
        }
    }

    #[test]
    fn ignores_non_images_entirely() {
        for ext in ["txt", "mp4", "zip", "db", "pdf", ""] {
            assert_eq!(classify(ext), None, "for {ext:?}");
        }
    }

    #[test]
    fn reads_extension_without_consulting_case() {
        assert_eq!(extension_of(&PathBuf::from("Photo.JPEG")), "jpeg");
        assert_eq!(extension_of(&PathBuf::from("archive.tar.gz")), "gz");
        assert_eq!(extension_of(&PathBuf::from("noext")), "");
    }

    #[test]
    fn mime_lookup_covers_external_formats() {
        assert_eq!(mime_for("heic"), "image/heic");
        assert_eq!(mime_for("avif"), "image/avif");
        assert_eq!(mime_for("unknown"), "application/octet-stream");
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("envision-images-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Create JPEG bytes with a `.png` name to verify content-based decoding.
    fn write_jpeg_named_png(dir: &Path, name: &str, width: u32, height: u32) -> PathBuf {
        let path = dir.join(name);
        image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(
            width,
            height,
            vec![200u8; (width * height * 3) as usize],
        )
        .unwrap())
        .save_with_format(&path, image::ImageFormat::Jpeg)
        .unwrap();
        path
    }

    #[test]
    fn decodes_a_jpeg_that_claims_to_be_a_png() {
        let dir = temp_dir("mislabelled");
        let path = write_jpeg_named_png(&dir, "liar.png", 64, 32);

        // Sanity: the bytes really are a JPEG, and really do carry a .png name.
        let magic = std::fs::read(&path).unwrap();
        assert_eq!(&magic[..2], &[0xff, 0xd8], "written as JPEG");
        assert_eq!(path.extension().unwrap(), "png");

        // The extension-based entry point rejects it outright.
        assert!(
            image::image_dimensions(&path).is_err(),
            "image::image_dimensions trusts the extension and must still fail — \
             this test is only meaningful because the old path was wrong"
        );

        // Ours does not.
        assert_eq!(dimensions(&path), Some((64, 32)));

        let decoded = open_by_content(&path).expect("content-based decode");
        assert_eq!((decoded.width(), decoded.height()), (64, 32));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn content_decode_still_handles_files_whose_extension_is_truthful() {
        let dir = temp_dir("honest");
        let path = dir.join("honest.png");
        image::RgbImage::from_raw(40, 20, vec![7u8; 40 * 20 * 3])
            .unwrap()
            .save(&path)
            .unwrap();

        assert_eq!(dimensions(&path), Some((40, 20)));
        assert_eq!(open_by_content(&path).unwrap().width(), 40);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn unreadable_files_report_no_dimensions_rather_than_panicking() {
        let dir = temp_dir("garbage");
        let path = dir.join("not-an-image.png");
        std::fs::write(&path, b"this is not an image at all").unwrap();

        assert_eq!(dimensions(&path), None);
        assert!(open_by_content(&path).is_err());

        std::fs::remove_dir_all(&dir).ok();
    }
}