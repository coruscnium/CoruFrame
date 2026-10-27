//! Shared WFCD item-art cache.
//!
//! Both the probe (prefetch during the mission) and the HUD (first paint) need
//! the same files. Downloads live here so the probe can warm the cache without
//! linking GTK and the HUD keeps its one lookup path.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::fetch;
use crate::private_fs;

pub const IMAGE_TIMEOUT: Duration = Duration::from_secs(10);

pub fn dir() -> PathBuf {
    fetch::cache_dir().join("hud")
}

pub fn safe_image(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains("..")
        && !name.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(
                    byte,
                    b'/' | b'\\' | b'?' | b'#' | b'%' | b':' | b'"' | b'<' | b'>' | b' '
                )
        })
}

// The HUD uses this: it never fetches, so a missing image is a placeholder
// until the probe's prefetch has warmed the cache.
pub fn cached(name: &str) -> Option<PathBuf> {
    if !safe_image(name) {
        eprintln!("art: unsafe image name {name:?}");
        return None;
    }
    let path = dir().join(name);
    path.is_file().then_some(path)
}

pub fn path(name: &str) -> Option<PathBuf> {
    if !safe_image(name) {
        eprintln!("art: unsafe image name {name:?}");
        return None;
    }
    let path = dir().join(name);
    if path.is_file() {
        return Some(path);
    }
    download(name, &path).then_some(path)
}

fn download(name: &str, path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        if let Err(error) = private_fs::ensure_private_dir(parent) {
            eprintln!("art: {error}");
            return false;
        }
    }
    let url = format!("https://cdn.warframestat.us/img/{name}");
    let timeout = IMAGE_TIMEOUT.as_secs().to_string();
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{file_name}.{}.tmp", std::process::id()));
    let output = std::process::Command::new("curl")
        .args([
            "-L",
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            timeout.as_str(),
            "-o",
        ])
        .arg(&tmp)
        .arg(&url)
        .status();
    match output {
        Ok(status) if status.success() => {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
            std::fs::rename(&tmp, path).is_ok()
        }
        Ok(status) => {
            let _ = std::fs::remove_file(&tmp);
            eprintln!("art: {name}: curl exited with {status}");
            false
        }
        Err(error) => {
            let _ = std::fs::remove_file(&tmp);
            eprintln!("art: curl {name}: {error}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_names_stay_one_path_component() {
        assert!(safe_image("GenericGunPrimeStock.png"));
        assert!(!safe_image("../etc/passwd"));
        assert!(!safe_image("sub/dir.png"));
        assert!(!safe_image("https://evil.example/x.png"));
        assert!(!safe_image("with space.png"));
    }
}
