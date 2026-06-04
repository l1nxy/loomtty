use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};

const TEMP_SUBDIR: &str = "loomtty-clipboard";
const FILENAME_PREFIX: &str = "loomtty-paste-";

/// Maximum number of paste files retained in the temp directory.
/// At ~10 MB per screenshot, 50 caps the directory at a few hundred MB
/// even when the OS never cleans `%TEMP%` on its own (default on Windows).
const KEEP_RECENT: usize = 50;

/// Encode an `arboard` clipboard image to PNG inside the system temp dir
/// and return the absolute path of the written file. Older paste files
/// in the same directory are pruned (best-effort) so the directory stays
/// bounded.
pub(crate) fn save_to_temp(img: &arboard::ImageData<'_>) -> std::io::Result<PathBuf> {
    save_to_dir(img, &std::env::temp_dir())
}

/// POSIX-style shell quoting matching `WindowEvent::DroppedFile` so a path
/// containing whitespace, backslashes or shell metacharacters survives the
/// PTY -> shell hop intact. Identical rule as the dropped-file handler.
pub(crate) fn quote_path_for_shell(path: &str) -> String {
    if path.contains(|c: char| c.is_whitespace() || "\"'\\$`!#&|;(){}[]<>?*~".contains(c)) {
        format!("'{}'", path.replace('\'', "'\\''"))
    } else {
        path.to_owned()
    }
}

fn save_to_dir(img: &arboard::ImageData<'_>, parent: &Path) -> std::io::Result<PathBuf> {
    let width = u32::try_from(img.width)
        .map_err(|_| invalid_input("image width overflows u32"))?;
    let height = u32::try_from(img.height)
        .map_err(|_| invalid_input("image height overflows u32"))?;
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|p| p.checked_mul(4))
        .ok_or_else(|| invalid_input("image dimensions overflow"))?;
    if img.bytes.len() != expected {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "clipboard image dimensions do not match byte buffer",
        ));
    }

    let dir = parent.join(TEMP_SUBDIR);
    std::fs::create_dir_all(&dir)?;

    let unix_ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let (file, path) = create_exclusive(&dir, unix_ts)?;
    {
        let writer = BufWriter::new(&file);
        PngEncoder::new(writer)
            .write_image(img.bytes.as_ref(), width, height, ExtendedColorType::Rgba8)
            .map_err(std::io::Error::other)?;
    }
    drop(file);

    let _ = prune_old_files(&dir, KEEP_RECENT);

    Ok(path)
}

fn invalid_input(msg: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, msg)
}

/// Open `{prefix}{ts}.png` with `CREATE_NEW` semantics (`O_EXCL` on Unix),
/// retrying with `-1`, `-2`, ... suffixes on collision. This is atomic at
/// the filesystem level — closes the check-then-act window that a separate
/// `exists()` probe leaves open on a shared temp directory.
fn create_exclusive(dir: &Path, unix_ts: u64) -> std::io::Result<(std::fs::File, PathBuf)> {
    let mut counter: u32 = 0;
    loop {
        let name = if counter == 0 {
            format!("{FILENAME_PREFIX}{unix_ts}.png")
        } else {
            format!("{FILENAME_PREFIX}{unix_ts}-{counter}.png")
        };
        let path = dir.join(name);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((file, path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                counter = counter
                    .checked_add(1)
                    .ok_or_else(|| std::io::Error::other("exhausted unique filename counter"))?;
            }
            Err(e) => return Err(e),
        }
    }
}

fn prune_old_files(dir: &Path, keep: usize) -> std::io::Result<()> {
    let mut entries: Vec<(PathBuf, SystemTime)> = std::fs::read_dir(dir)?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if !name_str.starts_with(FILENAME_PREFIX) || !name_str.ends_with(".png") {
                return None;
            }
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let mtime = meta.modified().ok()?;
            Some((entry.path(), mtime))
        })
        .collect();
    if entries.len() <= keep {
        return Ok(());
    }
    entries.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in entries.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::borrow::Cow;
    use tempfile::TempDir;

    #[test]
    fn quote_path_for_shell_passes_clean_paths_through() {
        assert_eq!(quote_path_for_shell("/tmp/clean.png"), "/tmp/clean.png");
        assert_eq!(
            quote_path_for_shell("C:/Users/foo/Temp/x.png"),
            "C:/Users/foo/Temp/x.png"
        );
    }

    #[test]
    fn quote_path_for_shell_wraps_whitespace_and_metachars() {
        assert_eq!(
            quote_path_for_shell("/tmp/has space.png"),
            "'/tmp/has space.png'"
        );
        assert_eq!(
            quote_path_for_shell(r"C:\Users\John Smith\Temp\a.png"),
            r"'C:\Users\John Smith\Temp\a.png'"
        );
        assert_eq!(
            quote_path_for_shell("/tmp/(par)ens.png"),
            "'/tmp/(par)ens.png'"
        );
    }

    #[test]
    fn quote_path_for_shell_escapes_embedded_single_quote() {
        // POSIX trick: close quote, emit \', reopen quote.
        assert_eq!(
            quote_path_for_shell("/tmp/it's.png"),
            "'/tmp/it'\\''s.png'"
        );
    }

    #[test]
    fn create_exclusive_appends_counter_on_collision() {
        let tmp = TempDir::new().unwrap();
        let ts = 1_700_000_000;
        let (_f1, p1) = create_exclusive(tmp.path(), ts).unwrap();
        let (_f2, p2) = create_exclusive(tmp.path(), ts).unwrap();
        let (_f3, p3) = create_exclusive(tmp.path(), ts).unwrap();
        assert_eq!(p1.file_name().unwrap(), "loomtty-paste-1700000000.png");
        assert_eq!(p2.file_name().unwrap(), "loomtty-paste-1700000000-1.png");
        assert_eq!(p3.file_name().unwrap(), "loomtty-paste-1700000000-2.png");
    }

    #[test]
    fn save_to_dir_writes_decodable_png() {
        let tmp = TempDir::new().unwrap();
        let pixels: Vec<u8> = vec![
            255, 0, 0, 255, // red
            0, 255, 0, 255, // green
            0, 0, 255, 255, // blue
            255, 255, 0, 255, // yellow
        ];
        let img = arboard::ImageData {
            width: 2,
            height: 2,
            bytes: Cow::Owned(pixels),
        };
        let path = save_to_dir(&img, tmp.path()).expect("save");
        assert!(path.exists());
        assert!(path.starts_with(tmp.path().join(TEMP_SUBDIR)));
        let decoded = image::open(&path).expect("decode png").to_rgba8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert_eq!(decoded.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(decoded.get_pixel(1, 1).0, [255, 255, 0, 255]);
    }

    #[test]
    fn save_to_dir_rejects_mismatched_buffer() {
        let tmp = TempDir::new().unwrap();
        let img = arboard::ImageData {
            width: 4,
            height: 4,
            bytes: Cow::Owned(vec![0u8; 8]),
        };
        let err = save_to_dir(&img, tmp.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn prune_keeps_newest_n_by_mtime() {
        let tmp = TempDir::new().unwrap();
        // Set mtimes explicitly via stdlib `File::set_modified` so the test
        // is deterministic across filesystems (FAT32, NTFS, ext4, APFS) and
        // does not depend on real wall-clock spacing.
        let base = SystemTime::now() - std::time::Duration::from_secs(3600);
        let mut paths = Vec::new();
        for i in 0..5u64 {
            let name = format!("{FILENAME_PREFIX}1000-{i}.png");
            let path = tmp.path().join(&name);
            std::fs::write(&path, b"x").unwrap();
            let f = std::fs::OpenOptions::new()
                .write(true)
                .open(&path)
                .unwrap();
            // i=0 oldest, i=4 newest — 10s apart, well above any FS resolution
            f.set_modified(base + std::time::Duration::from_secs(i * 10))
                .unwrap();
            paths.push(path);
        }
        // Unrelated files should be left alone.
        let stranger = tmp.path().join("not-a-paste.txt");
        std::fs::write(&stranger, b"keep").unwrap();

        prune_old_files(tmp.path(), 2).unwrap();

        assert!(!paths[0].exists(), "oldest should be pruned");
        assert!(!paths[1].exists());
        assert!(!paths[2].exists());
        assert!(paths[3].exists(), "second-newest should survive");
        assert!(paths[4].exists(), "newest should survive");
        assert!(stranger.exists(), "non-paste files must not be touched");
    }
}
