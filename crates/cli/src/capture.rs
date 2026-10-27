use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::ocr::run_timed_status;

static FRAME_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const CLI_TIMEOUT: Duration = Duration::from_secs(8);
const OVERRIDE_TIMEOUT: Duration = Duration::from_secs(10);
const PORTAL_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    // Detect the compositor; only a PID-verified game window is captured.
    Auto,
    // KWin ScreenShot2 through kdotool.
    KWin,
    // swaymsg/hyprctl geometry plus grim on the whole output, cropped in Rust.
    Wlr,
    // xdotool geometry plus maim/import.
    X11,
    // xdg-desktop-portal Screenshot: explicit only, and never OCR'd unless the
    // frame can be mapped to one output.
    Portal,
}

impl Backend {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "kwin" => Some(Self::KWin),
            "wlr" => Some(Self::Wlr),
            "x11" => Some(Self::X11),
            "portal" => Some(Self::Portal),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::KWin => "kwin",
            Self::Wlr => "wlr",
            Self::X11 => "x11",
            Self::Portal => "portal",
        }
    }
}

#[derive(Clone, Debug)]
pub struct CaptureResult {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Env {
    pub session_type: String,
    pub desktop: String,
    pub sway: bool,
    pub hyprland: bool,
}

impl Env {
    pub fn from_process() -> Self {
        Self {
            session_type: std::env::var("XDG_SESSION_TYPE").unwrap_or_default(),
            desktop: std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default(),
            sway: std::env::var_os("SWAYSOCK").is_some(),
            hyprland: std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Tools {
    pub kdotool: bool,
    pub grim: bool,
    pub swaymsg: bool,
    pub hyprctl: bool,
    pub xdotool: bool,
    pub maim: bool,
    pub import: bool,
}

impl Tools {
    pub fn from_path() -> Self {
        Self {
            kdotool: in_path("kdotool"),
            grim: in_path("grim"),
            swaymsg: in_path("swaymsg"),
            hyprctl: in_path("hyprctl"),
            xdotool: in_path("xdotool"),
            maim: in_path("maim"),
            import: in_path("import"),
        }
    }
}

// Pure so the matrix is testable without a live session. kdotool alone is not
// proof of KWin: it is only considered on a KDE session.
pub fn detect_from(env: &Env, tools: &Tools) -> Option<Backend> {
    let desktop = env.desktop.to_ascii_lowercase();
    if desktop.contains("kde") && tools.kdotool {
        return Some(Backend::KWin);
    }
    if env.sway && tools.swaymsg && tools.grim {
        return Some(Backend::Wlr);
    }
    if env.hyprland && tools.hyprctl && tools.grim {
        return Some(Backend::Wlr);
    }
    if env.session_type.eq_ignore_ascii_case("x11") && tools.xdotool && (tools.maim || tools.import)
    {
        return Some(Backend::X11);
    }
    None
}

pub fn detect() -> Option<Backend> {
    detect_from(&Env::from_process(), &Tools::from_path())
}

pub fn resolve(backend: Backend) -> Result<Backend, String> {
    match backend {
        Backend::Auto => detect().ok_or_else(|| {
            "no supported capture backend: need KWin+kdotool, sway/hyprland+grim, or x11+xdotool+maim. Run `coruframe doctor`.".to_string()
        }),
        other => Ok(other),
    }
}

// CORUFRAME_CAPTURE_CMD always wins: it receives the output path in CORUFRAME_OUT.
// It is trusted operator input and is documented as such.
pub fn capture(dir: &Path, backend: Backend) -> Result<CaptureResult, String> {
    coruframe_core::private_fs::ensure_private_dir(dir)
        .map_err(|error| format!("frames dir: {error}"))?;

    let result = if let Ok(command) = std::env::var("CORUFRAME_CAPTURE_CMD") {
        run_override(dir, &command)?
    } else {
        match resolve(backend)? {
            Backend::KWin => kwin(dir)?,
            Backend::Wlr => wlr(dir)?,
            Backend::X11 => x11(dir)?,
            Backend::Portal => portal(dir)?,
            Backend::Auto => unreachable!("resolve maps these"),
        }
    };
    // A frame shows whatever the game shows; keep it owner-only like the token.
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&result.path, std::fs::Permissions::from_mode(0o600));
    Ok(result)
}

fn kwin(dir: &Path) -> Result<CaptureResult, String> {
    let id = kwin_window().ok_or("Warframe window not found (PID-verified)")?;
    let path = frame_path(dir);
    crate::kwinshot::capture(&id, &path)?;
    let (width, height) = png_size(&path).ok_or("captured frame is not a readable PNG")?;
    Ok(CaptureResult {
        path,
        width,
        height,
    })
}

// Either shape is accepted: the game inside gamescope (KWin sees one window)
// and a plain Warframe window. Gamescope is preferred, but only when its
// process environment is actually the Warframe launch (AppID 230410).
fn kwin_window() -> Option<String> {
    if !in_path("kdotool") {
        return None;
    }
    for pattern in ["gamescope", "steam_app_230410", "warframe"] {
        for id in kdotool_search(pattern) {
            if kwin_window_is_warframe(&id) {
                return Some(id);
            }
        }
    }
    None
}

fn kdotool_search(pattern: &str) -> Vec<String> {
    let Ok(output) = Command::new("kdotool").args(["search", pattern]).output() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn kwin_window_is_warframe(id: &str) -> bool {
    let Ok(output) = Command::new("kdotool").args(["getwindowpid", id]).output() else {
        return false;
    };
    let Ok(pid) = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
    else {
        return false;
    };
    let cmdline = read_proc(pid, "cmdline");
    let environ = read_proc(pid, "environ");
    process_is_warframe(&cmdline, &environ)
}

fn read_proc(pid: u32, name: &str) -> String {
    std::fs::read(format!("/proc/{pid}/{name}"))
        .map(|bytes| String::from_utf8_lossy(&bytes).replace('\0', " "))
        .unwrap_or_default()
}

// The command line is the reliable signal for a plain window; a gamescope window
// is owned by the wrapper, whose environment names the Steam AppID instead.
pub fn process_is_warframe(cmdline: &str, environ: &str) -> bool {
    let command = cmdline.to_ascii_lowercase();
    let executable = command.split_whitespace().any(|token| {
        let name = token
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(token)
            .trim_matches('"');
        matches!(name, "warframe" | "warframe.exe" | "warframe.x64.exe")
    });
    if executable {
        return true;
    }
    let environment = environ.to_ascii_lowercase();
    environment.contains("steam_appid=230410")
        || environment.contains("steamappid=230410")
        || environment.contains("steam_compat_app_id=230410")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Rect {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

fn wlr(dir: &Path) -> Result<CaptureResult, String> {
    let env = Env::from_process();
    let window = if env.hyprland && in_path("hyprctl") {
        let clients = run_json("hyprctl", &["clients", "-j"])?;
        let monitors = run_json("hyprctl", &["monitors", "-j"])?;
        hyprland_window(&clients, &monitors)
    } else if env.sway && in_path("swaymsg") {
        let tree = run_json("swaymsg", &["-t", "get_tree"])?;
        let outputs = run_json("swaymsg", &["-t", "get_outputs"])?;
        sway_window(&tree, &outputs)
    } else {
        None
    };
    let (pid, window) = window.ok_or("no Warframe window on this compositor")?;
    if !pid_is_warframe(pid) {
        return Err("the matching compositor window is not the Warframe process".into());
    }
    let output = window
        .output
        .clone()
        .ok_or("could not resolve the window's output")?;
    let full = frame_path(dir);
    let status = run_timed_status(
        Command::new("grim").args(["-o", &output]).arg(&full),
        CLI_TIMEOUT,
    )?;
    if !status.success() || !full.is_file() {
        let _ = std::fs::remove_file(&full);
        return Err(format!("grim failed for output {output}"));
    }
    let crop = window.local_crop();
    let path = frame_path(dir);
    crop_png(&full, &path, crop)?;
    let _ = std::fs::remove_file(&full);
    Ok(CaptureResult {
        path,
        width: crop.width,
        height: crop.height,
    })
}

#[derive(Clone, Debug)]
struct WlrWindow {
    rect: Rect,
    output_origin: (i32, i32),
    scale: f64,
    output: Option<String>,
}

impl WlrWindow {
    // The window rect is layout/logical; grim writes the output at physical size.
    fn local_crop(&self) -> Rect {
        let x = ((self.rect.x - self.output_origin.0) as f64 * self.scale).round() as i32;
        let y = ((self.rect.y - self.output_origin.1) as f64 * self.scale).round() as i32;
        Rect {
            x: x.max(0),
            y: y.max(0),
            width: (self.rect.width as f64 * self.scale).round() as u32,
            height: (self.rect.height as f64 * self.scale).round() as u32,
        }
    }
}

fn sway_window(tree: &serde_json::Value, outputs: &serde_json::Value) -> Option<(u32, WlrWindow)> {
    let mut found = None;
    walk_sway(tree, &mut found);
    let node = found?;
    let pid = node.get("pid").and_then(serde_json::Value::as_u64)? as u32;
    let rect = json_rect(node.get("rect")?)?;
    let name = node.get("output").and_then(serde_json::Value::as_str)?;
    let (origin, scale) = outputs
        .as_array()?
        .iter()
        .find(|output| output.get("name").and_then(serde_json::Value::as_str) == Some(name))
        .and_then(|output| {
            let rect = json_rect(output.get("rect")?)?;
            let scale = output
                .get("scale")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0);
            Some(((rect.x, rect.y), scale))
        })
        .unwrap_or(((0, 0), 1.0));
    Some((
        pid,
        WlrWindow {
            rect,
            output_origin: origin,
            scale,
            output: Some(name.to_string()),
        },
    ))
}

fn walk_sway<'a>(node: &'a serde_json::Value, found: &mut Option<&'a serde_json::Value>) {
    if found.is_some() {
        return;
    }
    if node
        .get("pid")
        .and_then(serde_json::Value::as_u64)
        .is_some()
        && node.get("rect").is_some()
        && sway_app_matches(node)
    {
        *found = Some(node);
        return;
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(serde_json::Value::as_array) {
            for child in children {
                walk_sway(child, found);
            }
        }
    }
}

fn sway_app_matches(node: &serde_json::Value) -> bool {
    let app_id = node
        .get("app_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let class = node
        .get("window_properties")
        .and_then(|properties| properties.get("class"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let title = node
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    app_id.contains("warframe")
        || app_id.contains("steam_app_230410")
        || class.contains("warframe")
        || class.contains("steam_app_230410")
        || title.starts_with("warframe")
}

fn hyprland_window(
    clients: &serde_json::Value,
    monitors: &serde_json::Value,
) -> Option<(u32, WlrWindow)> {
    let clients = clients.as_array()?;
    for client in clients {
        let maps = client
            .get("mapped")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let hidden = client
            .get("hidden")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        if !maps || hidden {
            continue;
        }
        let class = client
            .get("class")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let initial = client
            .get("initialClass")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let title = client
            .get("title")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_ascii_lowercase();
        let named = class.contains("warframe")
            || class.contains("steam_app_230410")
            || initial.contains("warframe")
            || initial.contains("steam_app_230410")
            || title.starts_with("warframe");
        if !named {
            continue;
        }
        let pid = client.get("pid").and_then(serde_json::Value::as_u64)? as u32;
        let at = client.get("at")?.as_array()?;
        let size = client.get("size")?.as_array()?;
        let rect = Rect {
            x: at.first()?.as_i64()? as i32,
            y: at.get(1)?.as_i64()? as i32,
            width: size.first()?.as_i64()? as u32,
            height: size.get(1)?.as_i64()? as u32,
        };
        let monitor = client
            .get("monitor")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(-1);
        let (name, origin, scale) = monitors
            .as_array()
            .and_then(|monitors| {
                monitors.iter().find(|entry| {
                    entry.get("id").and_then(serde_json::Value::as_i64) == Some(monitor)
                })
            })
            .and_then(|monitor| {
                Some((
                    monitor.get("name")?.as_str()?.to_string(),
                    (
                        monitor.get("x")?.as_i64()? as i32,
                        monitor.get("y")?.as_i64()? as i32,
                    ),
                    monitor
                        .get("scale")
                        .and_then(serde_json::Value::as_f64)
                        .unwrap_or(1.0),
                ))
            })
            .unwrap_or(("".to_string(), (0, 0), 1.0));
        return Some((
            pid,
            WlrWindow {
                rect,
                output_origin: origin,
                scale,
                output: Some(name),
            },
        ));
    }
    None
}

fn json_rect(value: &serde_json::Value) -> Option<Rect> {
    Some(Rect {
        x: value.get("x")?.as_i64()? as i32,
        y: value.get("y")?.as_i64()? as i32,
        width: value.get("width")?.as_i64()? as u32,
        height: value.get("height")?.as_i64()? as u32,
    })
}

fn pid_is_warframe(pid: u32) -> bool {
    process_is_warframe(&read_proc(pid, "cmdline"), &read_proc(pid, "environ"))
}

fn x11(dir: &Path) -> Result<CaptureResult, String> {
    let (id, _rect) = x11_window().ok_or("no PID-verified Warframe window on X11")?;
    let path = frame_path(dir);
    if in_path("maim") {
        let status = run_timed_status(
            Command::new("maim").args(["-i", &id]).arg(&path),
            CLI_TIMEOUT,
        )?;
        if !status.success() {
            let _ = std::fs::remove_file(&path);
            return Err(format!("maim failed for window {id}"));
        }
    } else if in_path("import") {
        let status = run_timed_status(
            Command::new("import").args(["-window", &id]).arg(&path),
            CLI_TIMEOUT,
        )?;
        if !status.success() {
            let _ = std::fs::remove_file(&path);
            return Err(format!("import failed for window {id}"));
        }
    } else {
        return Err("no image tool: install maim or ImageMagick (import)".into());
    }
    let (width, height) = png_size(&path).ok_or("captured frame is not a readable PNG")?;
    Ok(CaptureResult {
        path,
        width,
        height,
    })
}

fn x11_window() -> Option<(String, Rect)> {
    if !in_path("xdotool") {
        return None;
    }
    for (flag, pattern) in [
        ("--class", "steam_app_230410"),
        ("--class", "warframe"),
        ("--name", "Warframe"),
    ] {
        let Ok(output) = Command::new("xdotool")
            .args(["search", flag, pattern])
            .output()
        else {
            continue;
        };
        for id in String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|id| !id.is_empty())
        {
            if !x11_window_is_warframe(id) {
                continue;
            }
            if let Some(rect) = x11_geometry(id) {
                return Some((id.to_string(), rect));
            }
        }
    }
    None
}

fn x11_window_is_warframe(id: &str) -> bool {
    let Ok(output) = Command::new("xdotool").args(["getwindowpid", id]).output() else {
        return false;
    };
    let Ok(pid) = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
    else {
        return false;
    };
    pid_is_warframe(pid)
}

fn x11_geometry(id: &str) -> Option<Rect> {
    let output = Command::new("xdotool")
        .args(["getwindowgeometry", "--shell", id])
        .output()
        .ok()?;
    parse_x11_geometry(&String::from_utf8_lossy(&output.stdout))
}

fn parse_x11_geometry(text: &str) -> Option<Rect> {
    let mut rect = Rect::default();
    let mut seen = 0;
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key {
            "X" => {
                rect.x = value.parse().ok()?;
                seen += 1;
            }
            "Y" => {
                rect.y = value.parse().ok()?;
                seen += 1;
            }
            "WIDTH" => {
                rect.width = value.parse().ok()?;
                seen += 1;
            }
            "HEIGHT" => {
                rect.height = value.parse().ok()?;
                seen += 1;
            }
            _ => {}
        }
    }
    (seen == 4 && rect.width > 0 && rect.height > 0).then_some(rect)
}

fn run_json(program: &str, args: &[&str]) -> Result<serde_json::Value, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("{program}: {error}"))?;
    if !output.status.success() {
        return Err(format!("{program} exited {}", output.status));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("{program} json: {error}"))
}

fn crop_png(source: &Path, destination: &Path, crop: Rect) -> Result<(), String> {
    let image =
        image::open(source).map_err(|error| format!("open {}: {error}", source.display()))?;
    let width = image.width();
    let height = image.height();
    let x = (crop.x.max(0) as u32).min(width.saturating_sub(1));
    let y = (crop.y.max(0) as u32).min(height.saturating_sub(1));
    let right = (x + crop.width).min(width);
    let bottom = (y + crop.height).min(height);
    if right <= x || bottom <= y {
        return Err(format!(
            "window rect {}x{}+{}+{} is outside the {}x{} output",
            crop.width, crop.height, crop.x, crop.y, width, height
        ));
    }
    let cropped = image::imageops::crop_imm(&image, x, y, right - x, bottom - y).to_image();
    cropped
        .save(destination)
        .map_err(|error| format!("save {}: {error}", destination.display()))
}

fn portal(dir: &Path) -> Result<CaptureResult, String> {
    let runtime = tokio::runtime::Runtime::new().map_err(|error| error.to_string())?;
    let response = runtime.block_on(async {
        tokio::time::timeout(PORTAL_TIMEOUT, async {
            ashpd::desktop::screenshot::Screenshot::request()
                .interactive(false)
                .modal(false)
                .send()
                .await
                .map_err(|error| error.to_string())?
                .response()
                .map_err(|error| error.to_string())
        })
        .await
        .map_err(|_| "portal screenshot timed out after 15 s".to_string())?
    })?;
    let uri = response.uri();
    let source = url::Url::parse(&uri.to_string())
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .ok_or_else(|| format!("portal returned a non-file URI: {uri}"))?;
    let path = move_into(&source, dir)?;
    let (width, height) = png_size(&path).ok_or("portal frame is not a readable PNG")?;
    Ok(CaptureResult {
        path,
        width,
        height,
    })
}

fn run_override(dir: &Path, command: &str) -> Result<CaptureResult, String> {
    let path = frame_path(dir);
    let mut child = Command::new("sh");
    child.arg("-c").arg(command).env("CORUFRAME_OUT", &path);
    let status = run_timed_status(&mut child, OVERRIDE_TIMEOUT)
        .map_err(|error| format!("CORUFRAME_CAPTURE_CMD: {error}"))?;
    if !status.success() || !path.is_file() {
        return Err(format!("CORUFRAME_CAPTURE_CMD failed: {command}"));
    }
    let (width, height) = png_size(&path).ok_or("CORUFRAME_CAPTURE_CMD did not write a PNG")?;
    Ok(CaptureResult {
        path,
        width,
        height,
    })
}

fn move_into(source: &Path, dir: &Path) -> Result<PathBuf, String> {
    let destination = frame_path(dir);
    if std::fs::rename(source, &destination).is_err() {
        std::fs::copy(source, &destination).map_err(|error| format!("copy: {error}"))?;
        let _ = std::fs::remove_file(source);
    }
    Ok(destination)
}

fn frame_path(dir: &Path) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let sequence = FRAME_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    dir.join(format!(
        "{}.{:03}-{sequence}.png",
        now.as_secs(),
        now.subsec_millis()
    ))
}

pub fn png_size(path: &Path) -> Option<(u32, u32)> {
    use std::io::Read;
    let mut header = [0u8; 24];
    std::fs::File::open(path)
        .ok()?
        .read_exact(&mut header)
        .ok()?;
    if &header[..8] != b"\x89PNG\r\n\x1a\n" || &header[12..16] != b"IHDR" {
        return None;
    }
    Some((
        u32::from_be_bytes(header[16..20].try_into().ok()?),
        u32::from_be_bytes(header[20..24].try_into().ok()?),
    ))
}

fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|directory| directory.join(program).is_file())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_paths_are_unique() {
        let dir = std::env::temp_dir();
        let first = frame_path(&dir);
        let second = frame_path(&dir);
        assert!(first.to_string_lossy().ends_with(".png"));
        assert_ne!(first, second);
    }

    #[test]
    fn verifies_the_game_process_not_the_window_title() {
        assert!(process_is_warframe(
            "/home/x/Warframe.x64.exe -some-arg",
            ""
        ));
        assert!(process_is_warframe("", "SteamAppId=230410"));
        assert!(process_is_warframe("", "STEAM_COMPAT_APP_ID=230410"));
        assert!(!process_is_warframe("/usr/bin/firefox", "DISPLAY=:0"));
        assert!(!process_is_warframe("/usr/bin/code", ""));
        assert!(!process_is_warframe("", "SteamAppId=440"));
    }

    #[test]
    fn detects_backends_from_env_and_tools() {
        let tools = Tools {
            kdotool: true,
            grim: true,
            swaymsg: true,
            hyprctl: true,
            xdotool: true,
            maim: true,
            import: true,
        };
        let env = Env {
            session_type: "wayland".into(),
            desktop: "KDE".into(),
            sway: false,
            hyprland: false,
        };
        assert_eq!(detect_from(&env, &tools), Some(Backend::KWin));

        let env = Env {
            session_type: "wayland".into(),
            desktop: "sway".into(),
            sway: true,
            hyprland: false,
        };
        assert_eq!(detect_from(&env, &tools), Some(Backend::Wlr));

        let env = Env {
            session_type: "x11".into(),
            desktop: "GNOME".into(),
            sway: false,
            hyprland: false,
        };
        assert_eq!(detect_from(&env, &tools), Some(Backend::X11));

        let none = Tools::default();
        assert_eq!(detect_from(&env, &none), None);
    }

    #[test]
    fn parses_sway_window_and_output() {
        let tree = serde_json::json!({
            "nodes": [{
                "nodes": [{
                    "pid": 42,
                    "app_id": "steam_app_230410",
                    "name": "Warframe",
                    "output": "DP-1",
                    "rect": {"x": 100, "y": 50, "width": 1200, "height": 800}
                }],
                "floating_nodes": []
            }]
        });
        let outputs = serde_json::json!([{
            "name": "DP-1",
            "rect": {"x": 0, "y": 0, "width": 1920, "height": 1080},
            "scale": 2.0
        }]);
        let (pid, window) = sway_window(&tree, &outputs).unwrap();
        assert_eq!(pid, 42);
        let crop = window.local_crop();
        assert_eq!(
            (crop.x, crop.y, crop.width, crop.height),
            (200, 100, 2400, 1600)
        );
        assert_eq!(window.output.as_deref(), Some("DP-1"));
    }

    #[test]
    fn parses_hyprland_window_and_monitor() {
        let clients = serde_json::json!([{
            "pid": 42,
            "class": "steam_app_230410",
            "title": "Warframe",
            "at": [200, 100],
            "size": [1200, 800],
            "monitor": 1,
            "mapped": true
        }]);
        let monitors = serde_json::json!([{
            "id": 1,
            "name": "DP-2",
            "x": 1920,
            "y": 0,
            "width": 1920,
            "height": 1080,
            "scale": 1.0
        }]);
        crate::capture::tests::assert_hyprland(clients, monitors);
    }

    pub(super) fn assert_hyprland(clients: serde_json::Value, monitors: serde_json::Value) {
        let (pid, window) = hyprland_window(&clients, &monitors).unwrap();
        assert_eq!(pid, 42);
        assert_eq!((window.rect.x, window.rect.y), (200, 100));
        assert_eq!(window.output.as_deref(), Some("DP-2"));
        assert_eq!(window.output_origin, (1920, 0));
    }

    #[test]
    fn parses_x11_geometry() {
        let text = "WINDOW=123\nX=10\nY=20\nWIDTH=1920\nHEIGHT=1080\nSCREEN=0\n";
        assert_eq!(
            parse_x11_geometry(text).map(|r| (r.x, r.y, r.width, r.height)),
            Some((10, 20, 1920, 1080))
        );
        assert_eq!(parse_x11_geometry("WINDOW=123\n"), None);
    }
}
