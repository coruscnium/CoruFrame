use std::os::unix::fs::PermissionsExt;

use coruframe_core::capabilities::{self, Capabilities};
use coruframe_core::private_fs;

use crate::snapshot;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run(args: &[String]) -> Result<()> {
    let report = match args {
        [] => false,
        [flag] if flag == "--report" => true,
        [other, ..] => return Err(format!("unknown option: {other}").into()),
    };
    let mut caps = capabilities::detect();
    caps.layer_shell = layer_shell();
    if report {
        // The capability document holds no keys or paths, so only bare
        // addresses and tokens need redaction; keyed redaction hides the
        // `session_type` and `secret_service` diagnostics themselves.
        let json = serde_json::to_string_pretty(&caps)?;
        println!("{}", snapshot::redact_bare(&json));
        return Ok(());
    }
    print_human(&caps);
    Ok(())
}

fn print_human(caps: &Capabilities) {
    println!(
        "session: {} ({})",
        if caps.session_type.is_empty() {
            "unknown"
        } else {
            &caps.session_type
        },
        if caps.desktop.is_empty() {
            "no XDG_CURRENT_DESKTOP"
        } else {
            &caps.desktop
        }
    );
    println!(
        "wayland: {}",
        if caps.wayland {
            "yes"
        } else {
            "no; CoruFrame is Wayland-only and refuses to start"
        }
    );
    println!("package: {}", caps.package);
    println!("portal: {}", portal_state(&caps.portal));
    println!(
        "portal backend: {}",
        caps.portal.backend.as_deref().unwrap_or("unknown")
    );
    println!(
        "pipewire: {}",
        if caps.pipewire {
            "present"
        } else {
            "missing; capture off"
        }
    );
    println!(
        "ocr: {}",
        if !crate::ocr::available() {
            "tesseract missing; reward OCR is off"
        } else if !crate::ocr::eng_available() {
            "tesseract present, eng model missing; reward OCR is off"
        } else {
            "tesseract + eng present"
        }
    );
    println!(
        "layer-shell: {}",
        match caps.layer_shell {
            Some(true) => "supported",
            Some(false) => "missing; no reward HUD on this compositor",
            None => "unknown; GTK could not open a display",
        }
    );
    println!(
        "secret service: {}",
        if caps.secret_service {
            "org.freedesktop.secrets present".to_string()
        } else {
            "absent; the token falls back to a 0600 file".to_string()
        }
    );
    println!(
        "tesseract: {}",
        caps.tesseract
            .as_deref()
            .unwrap_or("missing; OCR and the reward HUD are off")
    );
    println!("snapshot: {}", snapshot_state(caps));
    println!(
        "capture token: {}",
        if caps.capture_token {
            "present"
        } else {
            "absent; no ScreenCast grant stored"
        }
    );
    for (label, path) in [
        ("cache", coruframe_core::fetch::cache_dir()),
        ("config", private_fs::config_dir()),
        ("data", private_fs::data_dir()),
    ] {
        println!("{label}: {} {}", path.display(), mode_of(&path));
    }
}

fn portal_state(portal: &capabilities::Portal) -> String {
    if !portal.present {
        return match &portal.error {
            Some(error) => format!("absent ({error}); capture off"),
            None => "absent; capture off".to_string(),
        };
    }
    let version = portal
        .version
        .map(|version| format!("version {version}"))
        .unwrap_or_else(|| "version unknown".to_string());
    let mut sources = Vec::new();
    if portal.monitor_source {
        sources.push("monitor");
    }
    if portal.window_source {
        sources.push("window");
    }
    let sources = if sources.is_empty() {
        "no sources".to_string()
    } else {
        sources.join("+")
    };
    match &portal.error {
        Some(error) => format!("present, {version}, {sources} ({error})"),
        None => format!("present, {version}, {sources}"),
    }
}

fn snapshot_state(caps: &Capabilities) -> String {
    let snapshot = &caps.snapshot;
    let state = if snapshot.present {
        "present"
    } else {
        "absent"
    };
    let scope = snapshot
        .ptrace_scope
        .map(|scope| format!("ptrace_scope {scope}"))
        .unwrap_or_else(|| "ptrace_scope unavailable".to_string());
    if snapshot.blocked {
        format!(
            "{state}; blocked: {scope} needs CAP_SYS_PTRACE, so the wrapper cannot read the game"
        )
    } else {
        format!("{state}; {scope}")
    }
}

pub(crate) fn layer_shell() -> Option<bool> {
    if gtk4::init().is_err() {
        return None;
    }
    Some(gtk4_layer_shell::is_supported())
}

fn mode_of(path: &std::path::Path) -> String {
    std::fs::symlink_metadata(path)
        .map(|meta| {
            let kind = if meta.file_type().is_symlink() {
                "symlink"
            } else if meta.is_dir() {
                "dir"
            } else {
                "file"
            };
            format!("{kind} {:03o}", meta.permissions().mode() & 0o777)
        })
        .unwrap_or_else(|_| "missing".to_string())
}
