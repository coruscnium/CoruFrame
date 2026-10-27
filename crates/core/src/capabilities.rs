//! What this system can do, read without opening a portal dialog.
//!
//! Every probe here is read-only: D-Bus property reads, a library lookup, a
//! version command. The capture permission flow lives in the CLI, not here.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::private_fs;

const DBUS_TIMEOUT: Duration = Duration::from_secs(3);

pub const CAPTURE_TOKEN_FILE: &str = "capture-token";

pub fn capture_token_path() -> std::path::PathBuf {
    private_fs::config_dir().join(CAPTURE_TOKEN_FILE)
}

// Portal ScreenCast source bits (xdg-desktop-portal ScreenCast interface).
const SOURCE_MONITOR: u32 = 1;
const SOURCE_WINDOW: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Portal {
    pub present: bool,
    pub version: Option<u32>,
    pub source_types: Option<u32>,
    pub monitor_source: bool,
    pub window_source: bool,
    pub backend: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SnapshotCapability {
    pub present: bool,
    pub ptrace_scope: Option<u32>,
    pub blocked: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Capabilities {
    pub wayland: bool,
    pub session_type: String,
    pub desktop: String,
    pub package: String,
    pub portal: Portal,
    pub pipewire: bool,
    // Only a GTK process can answer this; the CLI fills it in.
    pub layer_shell: Option<bool>,
    pub secret_service: bool,
    pub tesseract: Option<String>,
    pub snapshot: SnapshotCapability,
    pub capture_token: bool,
}

pub fn detect() -> Capabilities {
    let session_type = env("XDG_SESSION_TYPE");
    let desktop = env("XDG_CURRENT_DESKTOP");
    let (portal, secret_service) = dbus_probe_bounded(&desktop);
    Capabilities {
        wayland: effective_display().is_some(),
        session_type,
        desktop,
        package: package_flavour().to_string(),
        portal,
        pipewire: pipewire_available(),
        layer_shell: None,
        secret_service,
        tesseract: tesseract_version(),
        snapshot: snapshot_capability(),
        capture_token: capture_token_path().is_file(),
    }
}

// The app is Wayland-only; X11/XWayland is refused before anything starts.
pub fn require_wayland() -> Result<(), String> {
    wayland_gate(effective_display().as_deref())
}

// Proton games and gamescope clients may be Xwayland; that is fine for the
// game. The app and HUD must be native Wayland, so promote the gamescope socket
// and force GDK before any GTK process starts.
pub fn prepare_wayland() {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    if wayland.as_deref().is_none_or(str::is_empty) {
        if let Ok(display) = std::env::var("GAMESCOPE_WAYLAND_DISPLAY") {
            if !display.is_empty() {
                std::env::set_var("WAYLAND_DISPLAY", display);
            }
        }
    }
    if effective_display().is_some() {
        std::env::set_var("GDK_BACKEND", "wayland");
    }
}

fn wayland_gate(wayland_display: Option<&str>) -> Result<(), String> {
    match wayland_display {
        Some(display) if !display.is_empty() => Ok(()),
        _ => Err(
            "CoruFrame needs a Wayland session; an X11 session is not supported. \
             Proton games and gamescope clients may use Xwayland, but the app and HUD need \
             Wayland. Log in to a Wayland session (KDE Plasma, sway, Hyprland, ...)."
                .to_string(),
        ),
    }
}

pub fn package_flavour() -> &'static str {
    let appimage = std::env::var_os("APPIMAGE").is_some();
    let exe = std::env::current_exe().unwrap_or_default();
    flavour(appimage, &exe)
}

fn flavour(appimage: bool, exe: &Path) -> &'static str {
    if appimage {
        "appimage"
    } else if exe.starts_with("/nix/store") {
        "nix"
    } else {
        "dev"
    }
}

// yama scope 1 still lets the wrapper read its own child; 2 and 3 need
// CAP_SYS_PTRACE, which an unprivileged wrapper does not have.
fn ptrace_blocked(scope: Option<u32>) -> bool {
    scope.is_some_and(|value| value >= 2)
}

fn snapshot_capability() -> SnapshotCapability {
    let scope = ptrace_scope();
    SnapshotCapability {
        present: crate::fetch::cache_dir()
            .join("inventory-raw.json")
            .is_file(),
        ptrace_scope: scope,
        blocked: ptrace_blocked(scope),
    }
}

pub fn ptrace_scope() -> Option<u32> {
    std::fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope")
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

// The wrapper spawns the game, so it is the parent yama scope 1 allows; higher
// scopes need CAP_SYS_PTRACE and cannot read the child at all.
pub fn snapshot_block_reason() -> Option<String> {
    let scope = ptrace_scope()?;
    ptrace_blocked(Some(scope)).then(|| {
        format!(
            "kernel.yama.ptrace_scope={scope} needs CAP_SYS_PTRACE; \
             set it to 1 (a parent may trace its child) or 0 to use the snapshot"
        )
    })
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_default()
}

fn effective_display() -> Option<String> {
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let gamescope = std::env::var("GAMESCOPE_WAYLAND_DISPLAY").ok();
    effective_display_from(wayland.as_deref(), gamescope.as_deref())
}

// gamescope gives clients its nested Wayland socket under a different name and
// may leave WAYLAND_DISPLAY unset; it is still a Wayland compositor.
fn effective_display_from(wayland: Option<&str>, gamescope: Option<&str>) -> Option<String> {
    wayland
        .filter(|display| !display.is_empty())
        .or_else(|| gamescope.filter(|display| !display.is_empty()))
        .map(str::to_string)
}

fn source_flags(bits: Option<u32>) -> (bool, bool) {
    (
        bits.is_some_and(|value| value & SOURCE_MONITOR != 0),
        bits.is_some_and(|value| value & SOURCE_WINDOW != 0),
    )
}

// A hung or absent portal must not stall a capability read, so the blocking
// D-Bus work runs on its own thread with a deadline.
fn dbus_probe_bounded(desktop: &str) -> (Portal, bool) {
    let (tx, rx) = mpsc::channel();
    let desktop = desktop.to_string();
    std::thread::spawn(move || {
        let _ = tx.send(dbus_probe(&desktop));
    });
    rx.recv_timeout(DBUS_TIMEOUT).unwrap_or_else(|_| {
        (
            portal_absent(Some(format!(
                "org.freedesktop.portal.Desktop did not answer within {} s",
                DBUS_TIMEOUT.as_secs()
            ))),
            false,
        )
    })
}

fn dbus_probe(desktop: &str) -> (Portal, bool) {
    let connection = match zbus::blocking::Connection::session() {
        Ok(connection) => connection,
        Err(error) => {
            return (
                portal_absent(Some(format!("no session bus: {error}"))),
                false,
            )
        }
    };
    let secrets = name_has_owner(&connection, "org.freedesktop.secrets").unwrap_or(false);
    let portal = match name_has_owner(&connection, "org.freedesktop.portal.Desktop") {
        Ok(true) => portal_present(&connection, desktop),
        Ok(false) => portal_absent(None),
        Err(error) => portal_absent(Some(format!("portal probe: {error}"))),
    };
    (portal, secrets)
}

fn portal_absent(error: Option<String>) -> Portal {
    Portal {
        present: false,
        version: None,
        source_types: None,
        monitor_source: false,
        window_source: false,
        backend: None,
        error,
    }
}

fn portal_present(connection: &zbus::blocking::Connection, desktop: &str) -> Portal {
    let version = property_u32(connection, "version");
    let source_types = property_u32(connection, "AvailableSourceTypes");
    let (monitor_source, window_source) = source_flags(source_types);
    Portal {
        present: true,
        version,
        source_types,
        monitor_source,
        window_source,
        backend: portal_backend(desktop),
        error: source_types
            .is_none()
            .then(|| "ScreenCast properties unavailable".to_string()),
    }
}

fn name_has_owner(connection: &zbus::blocking::Connection, name: &str) -> Result<bool, String> {
    let reply = connection
        .call_method(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            Some("org.freedesktop.DBus"),
            "NameHasOwner",
            &(name,),
        )
        .map_err(|error| error.to_string())?;
    reply
        .body()
        .deserialize()
        .map_err(|error| error.to_string())
}

fn property_u32(connection: &zbus::blocking::Connection, property: &str) -> Option<u32> {
    let reply = connection
        .call_method(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.portal.ScreenCast", property),
        )
        .ok()?;
    let value: zbus::zvariant::OwnedValue = reply.body().deserialize().ok()?;
    u32::try_from(value).ok()
}

// portals.conf decides the backend; without one, xdg-desktop-portal falls back
// to the first installed .portal whose UseIn names this desktop.
fn portal_backend(desktop: &str) -> Option<String> {
    if let Some(path) = portal_conf_path() {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Some(name) = preferred_implementer(&text) {
                return Some(name);
            }
        }
    }
    installed_backend(desktop)
}

fn portal_conf_path() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        candidates.push(PathBuf::from(config).join("xdg-desktop-portal/portals.conf"));
    } else if let Some(home) = std::env::var_os("HOME") {
        candidates.push(PathBuf::from(home).join(".config/xdg-desktop-portal/portals.conf"));
    }
    candidates.push(PathBuf::from("/etc/xdg-desktop-portal/portals.conf"));
    for dir in data_dirs() {
        candidates.push(dir.join("xdg-desktop-portal/portals.conf"));
    }
    candidates.into_iter().find(|path| path.is_file())
}

fn preferred_implementer(text: &str) -> Option<String> {
    let mut section = "";
    let mut screen_cast = None;
    let mut default = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim();
            continue;
        }
        if section != "preferred" {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let Some(first) = value
            .split(';')
            .map(str::trim)
            .find(|name| !name.is_empty() && *name != "default")
        else {
            continue;
        };
        match key.trim() {
            "org.freedesktop.impl.portal.ScreenCast" => screen_cast = Some(first.to_string()),
            "default" => default = Some(first.to_string()),
            _ => {}
        }
    }
    screen_cast.or(default)
}

fn installed_backend(desktop: &str) -> Option<String> {
    let wanted: Vec<String> = desktop
        .split(':')
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    if wanted.is_empty() {
        return None;
    }
    let mut found = Vec::new();
    for dir in data_dirs() {
        let Ok(entries) = std::fs::read_dir(dir.join("xdg-desktop-portal/portals")) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("portal") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let use_in = ini_value(&text, "portal", "UseIn").unwrap_or_default();
            let interfaces = ini_value(&text, "portal", "Interfaces").unwrap_or_default();
            if !interfaces
                .split(';')
                .any(|name| name.trim() == "org.freedesktop.impl.portal.ScreenCast")
            {
                continue;
            }
            let matches = use_in
                .split(';')
                .map(|name| name.trim().to_ascii_lowercase())
                .any(|name| wanted.contains(&name));
            if !matches {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            found.push(stem.to_string());
        }
    }
    found.sort();
    found.into_iter().next()
}

fn data_dirs() -> Vec<PathBuf> {
    std::env::var_os("XDG_DATA_DIRS")
        .map(|dirs| std::env::split_paths(&dirs).collect())
        .unwrap_or_else(|| {
            vec![
                PathBuf::from("/usr/local/share"),
                PathBuf::from("/usr/share"),
            ]
        })
}

fn ini_value(text: &str, wanted_section: &str, key: &str) -> Option<String> {
    let mut section = "";
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            section = name.trim();
            continue;
        }
        if section != wanted_section {
            continue;
        }
        if let Some((name, value)) = line.split_once('=') {
            if name.trim() == key {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

fn pipewire_available() -> bool {
    ["libpipewire-0.3.so.0", "libpipewire-0.3.so"]
        .iter()
        .any(|name| dlopen_ok(name))
}

fn dlopen_ok(name: &str) -> bool {
    let Ok(name) = std::ffi::CString::new(name) else {
        return false;
    };
    // SAFETY: a NUL-terminated name and the standard dlopen flags.
    unsafe {
        let handle = libc::dlopen(name.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
        if handle.is_null() {
            return false;
        }
        libc::dlclose(handle);
        true
    }
}

fn tesseract_version() -> Option<String> {
    let output = Command::new("tesseract").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).into_owned()
    } else {
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    text.lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_portal_source_bits() {
        assert_eq!(source_flags(Some(7)), (true, true));
        assert_eq!(source_flags(Some(1)), (true, false));
        assert_eq!(source_flags(Some(2)), (false, true));
        assert_eq!(source_flags(None), (false, false));
    }

    #[test]
    fn a_parent_can_still_ptrace_under_scope_one() {
        assert!(!ptrace_blocked(Some(0)));
        assert!(!ptrace_blocked(Some(1)));
        assert!(ptrace_blocked(Some(2)));
        assert!(ptrace_blocked(Some(3)));
        assert!(!ptrace_blocked(None));
    }

    #[test]
    fn gamescope_supplies_the_wayland_display() {
        assert_eq!(
            effective_display_from(None, Some("gamescope-0")).as_deref(),
            Some("gamescope-0")
        );
        assert_eq!(
            effective_display_from(Some(""), Some("gamescope-0")).as_deref(),
            Some("gamescope-0")
        );
        assert_eq!(
            effective_display_from(Some("wayland-1"), Some("gamescope-0")).as_deref(),
            Some("wayland-1")
        );
        assert_eq!(effective_display_from(None, None), None);
    }

    #[test]
    fn wayland_needs_a_display() {
        assert!(wayland_gate(Some("wayland-1")).is_ok());
        assert!(wayland_gate(Some("")).is_err());
        assert!(wayland_gate(None).is_err());
    }

    #[test]
    fn the_package_flavour_follows_the_launch_path() {
        assert_eq!(flavour(true, Path::new("/nix/store/x/bin/app")), "appimage");
        assert_eq!(flavour(false, Path::new("/nix/store/x/bin/app")), "nix");
        assert_eq!(
            flavour(false, Path::new("/home/user/target/debug/app")),
            "dev"
        );
    }

    #[test]
    fn prefers_a_screencast_entry_over_the_default() {
        let text = "[preferred]\ndefault=gtk;\norg.freedesktop.impl.portal.ScreenCast=kde;\n";
        assert_eq!(preferred_implementer(text).as_deref(), Some("kde"));
        assert_eq!(
            preferred_implementer("[preferred]\ndefault=gtk;").as_deref(),
            Some("gtk")
        );
        assert_eq!(preferred_implementer("[other]\ndefault=gtk;"), None);
    }

    #[test]
    fn reads_portal_file_keys() {
        let text = "[portal]\nUseIn=KDE\nInterfaces=a;org.freedesktop.impl.portal.ScreenCast;b\n";
        assert_eq!(ini_value(text, "portal", "UseIn").as_deref(), Some("KDE"));
        assert!(ini_value(text, "portal", "Interfaces")
            .unwrap()
            .contains("ScreenCast"));
        assert_eq!(ini_value(text, "missing", "UseIn"), None);
    }
}
