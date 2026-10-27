use std::collections::HashMap;
use std::io::Read;
use std::os::fd::{FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};

use zbus::blocking::Connection;
use zbus::zvariant::{Fd, OwnedValue};

// A 16k x 16k frame at 4 bytes per pixel is already 1 GiB; the D-Bus reply is a
// window screenshot, so anything above this is a malformed reply, not a frame.
const MAX_DIMENSION: usize = 16_384;
const MAX_BYTES: usize = 512 << 20;

// KWin writes raw QImage pixels into a pipe; no focus and no gamescope protocol
// are involved, which is what makes this work while the game is under gamescope.
pub fn capture(uuid: &str, destination: &Path) -> Result<PathBuf, String> {
    let connection = Connection::session().map_err(|error| format!("dbus: {error}"))?;
    let mut fds = [0; 2];
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err("pipe2 failed".into());
    }
    // Both ends are owned from here on: no raw fd is ever closed twice.
    let read_end = unsafe { OwnedFd::from_raw_fd(fds[0]) };
    let write_end = unsafe { OwnedFd::from_raw_fd(fds[1]) };

    let mut options: HashMap<&str, OwnedValue> = HashMap::new();
    options.insert("include-shadow", OwnedValue::from(false));
    options.insert("include-decoration", OwnedValue::from(false));
    options.insert("native-resolution", OwnedValue::from(true));

    let reply = connection
        .call_method(
            Some("org.kde.KWin"),
            "/org/kde/KWin/ScreenShot2",
            Some("org.kde.KWin.ScreenShot2"),
            "CaptureWindow",
            &(uuid, &options, Fd::from(write_end)),
        )
        .map_err(|error| format!("CaptureWindow: {error}"))?;
    let body = reply
        .body()
        .deserialize::<HashMap<String, OwnedValue>>()
        .map_err(|error| format!("reply: {error}"))?;

    let (width, height, stride) = geometry(&body)?;
    let mut polled = libc::pollfd {
        fd: std::os::unix::io::AsRawFd::as_raw_fd(&read_end),
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut polled, 1, 8000) };
    if ready <= 0 {
        return Err("screenshot pipe timed out".into());
    }
    // `take` bounds the read even if KWin kept the pipe open past the header.
    let mut file = std::fs::File::from(read_end);
    let mut raw = Vec::new();
    file.by_ref()
        .take(stride as u64 * height as u64)
        .read_to_end(&mut raw)
        .map_err(|error| format!("pipe read: {error}"))?;
    if raw.len() < stride * height {
        return Err(format!(
            "short frame: {width}x{height} stride {stride}, {} bytes",
            raw.len()
        ));
    }

    let mut image = image::RgbImage::new(width as u32, height as u32);
    for y in 0..height {
        let row = &raw[y * stride..y * stride + width * 4];
        for (x, pixel) in row.as_chunks::<4>().0.iter().enumerate() {
            // QImage RGB32/ARGB32 are 0xAARRGGBB; little-endian bytes are B,G,R,A.
            image.put_pixel(
                x as u32,
                y as u32,
                image::Rgb([pixel[2], pixel[1], pixel[0]]),
            );
        }
    }
    let path = destination.to_path_buf();
    image
        .save(&path)
        .map_err(|error| format!("save {}: {error}", path.display()))?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    Ok(path)
}

fn number(body: &HashMap<String, OwnedValue>, key: &str) -> Result<usize, String> {
    body.get(key)
        .and_then(|value| u32::try_from(value).ok())
        .map(|value| value as usize)
        .ok_or_else(|| format!("missing {key}"))
}

fn geometry(body: &HashMap<String, OwnedValue>) -> Result<(usize, usize, usize), String> {
    let width = number(body, "width")?;
    let height = number(body, "height")?;
    let stride = number(body, "stride")?;
    validate_geometry(width, height, stride)?;
    Ok((width, height, stride))
}

fn validate_geometry(width: usize, height: usize, stride: usize) -> Result<(), String> {
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || stride < width * 4
        || stride > width * 4 + 64
    {
        return Err(format!(
            "implausible frame geometry: {width}x{height} stride {stride}"
        ));
    }
    let total = stride
        .checked_mul(height)
        .ok_or_else(|| "frame size overflow".to_string())?;
    if total > MAX_BYTES {
        return Err(format!("frame too large: {total} bytes"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_live_3440x1440_frame() {
        assert!(validate_geometry(3440, 1440, 3440 * 4).is_ok());
    }

    #[test]
    fn rejects_implausible_geometry() {
        assert!(validate_geometry(0, 1440, 0).is_err());
        assert!(validate_geometry(3440, 0, 13760).is_err());
        assert!(validate_geometry(3440, 1440, 100).is_err());
        assert!(validate_geometry(20_000, 1440, 80_000).is_err());
        assert!(validate_geometry(16_384, 16_384, 16_384 * 4 + 128).is_err());
        assert!(validate_geometry(usize::MAX / 2, 4, usize::MAX / 2).is_err());
    }
}
