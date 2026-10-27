use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

// Every private path goes through here so a new writer cannot quietly create a
// world-readable file next to one this module made 0600.
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config")
}

pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share")
}

fn xdg(variable: &str, fallback: &str) -> PathBuf {
    let base = std::env::var_os(variable)
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(fallback)))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("coruframe")
}

pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    if path.is_dir() {
        return Ok(());
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

// Write through a unique 0600 temp file and rename, so a reader never sees a
// half-written file and the mode is private before any content exists.
pub fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&tmp);
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.flush()?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp, path)
}

pub fn append_private(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut options = OpenOptions::new();
    options
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

pub fn create_private(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let mut options = OpenOptions::new();
    options
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(file)
}

// New writes are not enough: an older build left the cache 0755 and the snapshot
// 0644, so the known paths are corrected once at startup. Symlinks are never
// followed and anything outside the known list is left alone.
pub fn harden_existing_paths() -> Vec<String> {
    harden_paths(&crate::fetch::cache_dir(), &config_dir(), &data_dir())
}

pub fn harden_paths(cache: &Path, config: &Path, data: &Path) -> Vec<String> {
    let mut skipped = Vec::new();
    let cache = cache.to_path_buf();
    let mut dirs = vec![cache.clone(), config.to_path_buf(), data.to_path_buf()];
    dirs.push(cache.join("frames"));
    for dir in dirs {
        match fs::symlink_metadata(&dir) {
            Ok(meta) if meta.file_type().is_symlink() => {
                skipped.push(format!("{}: symlink, not followed", dir.display()))
            }
            Ok(meta) if meta.is_dir() => {
                if meta.permissions().mode() & 0o777 != 0o700 {
                    if let Err(error) = fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
                    {
                        skipped.push(format!("{}: {error}", dir.display()));
                    }
                }
            }
            Ok(_) => skipped.push(format!("{}: not a directory", dir.display())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => skipped.push(format!("{}: {error}", dir.display())),
        }
    }

    let config = config.to_path_buf();
    let mut files: Vec<PathBuf> = [
        "inventory-raw.json",
        "inventory-raw.partial.json",
        "inventory-scan.json",
        "inventory-status.json",
        "probe.log",
        "ledger.db",
        "ledger.db-journal",
        "ledger.db-wal",
        "ledger.db-shm",
    ]
    .iter()
    .map(|name| cache.join(name))
    .collect();
    for name in [
        "market-token",
        "market-device",
        "account-configured",
        "capture-token",
        "overlay.json",
    ] {
        files.push(config.join(name));
    }
    for dir in [cache.clone(), cache.join("frames")] {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            if is_frame_name_str(name)
                || (name.starts_with("overlay-") && name.ends_with(".json"))
                || name.starts_with("request-")
                || is_market_cache_name_str(name)
            {
                files.push(entry.path());
            }
        }
    }

    for path in files {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                skipped.push(format!("{}: symlink, not followed", path.display()))
            }
            Ok(meta) if meta.is_file() => {
                if meta.permissions().mode() & 0o777 != 0o600 {
                    if let Err(error) =
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                    {
                        skipped.push(format!("{}: {error}", path.display()));
                    }
                }
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => skipped.push(format!("{}: {error}", path.display())),
        }
    }
    skipped
}

// Market and catalog cache files whose content is account/session-adjacent:
// quotes, the item DB, per-item order books and their ETag/header companions.
fn is_market_cache_name_str(name: &str) -> bool {
    matches!(
        name,
        "quotes-v2.json" | "quotes.json" | "items.json" | "itemdb.json"
    ) || name.starts_with("orders-")
        || name.starts_with("top-")
        || name.starts_with("itemdb-")
        || name.ends_with(".etag")
        || name.ends_with(".headers")
}

// Exactly what `capture.rs` writes: <secs>.<3-digit millis>-<sequence>.png.
pub fn is_frame_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(is_frame_name_str)
}

pub fn is_frame_name_str(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".png") else {
        return false;
    };
    let Some((seconds, rest)) = stem.split_once('.') else {
        return false;
    };
    let Some((millis, sequence)) = rest.split_once('-') else {
        return false;
    };
    !seconds.is_empty()
        && seconds.bytes().all(|byte| byte.is_ascii_digit())
        && millis.len() == 3
        && millis.bytes().all(|byte| byte.is_ascii_digit())
        && !sequence.is_empty()
        && sequence.bytes().all(|byte| byte.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "coruframe-fs-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn writes_and_rewrites_owner_only() {
        let dir = temp("write");
        let path = dir.join("nested/inventory-raw.json");
        write_private(&path, b"{}").unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        write_private(&path, b"{\"a\":1}").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}");
        assert_eq!(
            fs::metadata(path.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn harden_corrects_known_modes_and_skips_symlinks() {
        let root = temp("harden");
        let cache = root.join("cache/coruframe");
        fs::create_dir_all(&cache).unwrap();
        fs::set_permissions(&cache, fs::Permissions::from_mode(0o755)).unwrap();
        let snapshot = cache.join("inventory-raw.json");
        fs::write(&snapshot, b"{}").unwrap();
        fs::set_permissions(&snapshot, fs::Permissions::from_mode(0o644)).unwrap();
        let frame = cache.join("1700000000.123-0.png");
        fs::write(&frame, b"x").unwrap();
        fs::set_permissions(&frame, fs::Permissions::from_mode(0o644)).unwrap();
        let quotes = cache.join("quotes-v2.json");
        fs::write(&quotes, b"{}").unwrap();
        fs::set_permissions(&quotes, fs::Permissions::from_mode(0o644)).unwrap();
        let order = cache.join("orders-fulmin_prime_barrel.json");
        fs::write(&order, b"{}").unwrap();
        fs::set_permissions(&order, fs::Permissions::from_mode(0o644)).unwrap();
        let unrelated = cache.join("notes.txt");
        fs::write(&unrelated, b"x").unwrap();
        fs::set_permissions(&unrelated, fs::Permissions::from_mode(0o644)).unwrap();
        std::os::unix::fs::symlink(&snapshot, root.join("link")).unwrap();

        harden_paths(
            &cache,
            &root.join("config/coruframe"),
            &root.join("data/coruframe"),
        );

        assert_eq!(
            fs::metadata(&cache).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(&snapshot).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&frame).unwrap().permissions().mode() & 0o777,
            0o600
        );
        for path in [&quotes, &order] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600,
                "{}",
                path.display()
            );
        }
        assert_eq!(
            fs::metadata(&unrelated).unwrap().permissions().mode() & 0o777,
            0o644
        );
        // Idempotent.
        assert!(harden_paths(
            &cache,
            &root.join("config/coruframe"),
            &root.join("data/coruframe")
        )
        .is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn recognizes_frame_names() {
        assert!(is_frame_name_str("1700000000.123-4.png"));
        assert!(!is_frame_name_str("1700000000.12-4.png"));
        assert!(!is_frame_name_str("1700000000.123-4.jpg"));
        assert!(!is_frame_name_str("../1700000000.123-4.png"));
    }
}
