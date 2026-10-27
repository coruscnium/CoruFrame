//! One supervised `coruframe probe --hud` child, owned by the app.
//!
//! The probe tails EE.log and spawns a short-lived HUD of its own. It is put in
//! its own process group so stopping it also stops an in-flight HUD instead of
//! leaving a 20 s orphan on screen.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;

use serde::Serialize;

use coruframe_core::private_fs;

const LOG_TAIL_LINES: usize = 80;

#[derive(Debug, Serialize)]
pub struct HudProbeStatus {
    pub enabled: bool,
    pub running: bool,
    pub pid: Option<u32>,
    pub log_path: String,
    pub last_error: Option<String>,
}

pub struct HudProbe {
    child: Mutex<Option<Child>>,
    last_error: Mutex<Option<String>>,
    resource_dir: Option<PathBuf>,
}

impl HudProbe {
    pub fn new(resource_dir: Option<PathBuf>) -> Self {
        Self {
            child: Mutex::new(None),
            last_error: Mutex::new(None),
            resource_dir,
        }
    }

    fn child(&self) -> std::sync::MutexGuard<'_, Option<Child>> {
        self.child.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn error(&self) -> std::sync::MutexGuard<'_, Option<String>> {
        self.last_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub fn status(&self) -> HudProbeStatus {
        let mut guard = self.child();
        let mut running = false;
        let mut pid = None;
        if let Some(child) = guard.as_mut() {
            match child.try_wait() {
                Ok(None) => {
                    running = true;
                    pid = Some(child.id());
                }
                Ok(Some(status)) => {
                    if !status.success() {
                        let message = format!(
                            "probe exited {}: {}",
                            status,
                            tail(LOG_TAIL_LINES).unwrap_or_else(|| "no log output".into())
                        );
                        *self.error() = Some(message);
                    }
                    *guard = None;
                }
                Err(error) => {
                    *self.error() = Some(format!("probe status: {error}"));
                    *guard = None;
                }
            }
        }
        HudProbeStatus {
            enabled: enabled_path().is_file(),
            running,
            pid,
            log_path: log_path().display().to_string(),
            last_error: self.error().clone(),
        }
    }

    pub fn start(&self, cli: &Path) -> Result<(), String> {
        {
            let mut guard = self.child();
            if let Some(child) = guard.as_mut() {
                match child.try_wait() {
                    Ok(None) => return Ok(()),
                    _ => *guard = None,
                }
            }
        }
        let log = log_path();
        let stderr = private_fs::append_private(&log)
            .map_err(|error| format!("probe log {}: {error}", log.display()))?;
        let mut command = Command::new(cli);
        command
            .args(["probe", "--hud", "--log-file"])
            .arg(&log)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr));
        add_bundled_runtime(&mut command, self.resource_dir.as_deref());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        private_fs::write_private(&enabled_path(), b"1")
            .map_err(|error| format!("save HUD setting: {error}"))?;
        let child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let _ = std::fs::remove_file(enabled_path());
                return Err(format!("start {}: {error}", cli.display()));
            }
        };
        *self.error() = None;
        *self.child() = Some(child);
        Ok(())
    }

    pub fn stop(&self) {
        let mut guard = self.child();
        if let Some(mut child) = guard.take() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(child.id() as i32), libc::SIGTERM);
            }
            let _ = child.wait();
        }
        let _ = std::fs::remove_file(enabled_path());
    }

    pub fn enabled(&self) -> bool {
        enabled_path().is_file()
    }
}

fn enabled_path() -> PathBuf {
    private_fs::config_dir().join("hud-enabled")
}

pub fn log_path() -> PathBuf {
    coruframe_core::fetch::cache_dir().join("probe.log")
}

pub fn log_tail() -> String {
    tail(LOG_TAIL_LINES).unwrap_or_default()
}

fn tail(lines: usize) -> Option<String> {
    tail_path(&log_path(), lines)
}

fn tail_path(path: &Path, lines: usize) -> Option<String> {
    if lines == 0 {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let tail: Vec<&str> = text.lines().rev().take(lines).collect();
    if tail.is_empty() {
        return None;
    }
    Some(tail.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

// Enabling is refused with the same reasons the capabilities panel shows, so
// the user does not get a probe that exits a second later.
pub fn gate(layer_shell: Option<bool>, tesseract: Option<&str>) -> Result<(), String> {
    if layer_shell != Some(true) {
        return Err(if layer_shell == Some(false) {
            "The compositor has no wlr-layer-shell; the reward HUD cannot draw here.".into()
        } else {
            "Could not check wlr-layer-shell; open Settings → This system.".into()
        });
    }
    if tesseract.is_none() {
        return Err("Tesseract with the English model is missing; the reward HUD needs it.".into());
    }
    Ok(())
}

// A packaged app carries Tesseract data and the binary. Point the child at the
// bundled copy without touching the user environment outside the probe.
fn add_bundled_runtime(command: &mut Command, resource_dir: Option<&Path>) {
    let Some(resource_dir) = resource_dir else {
        return;
    };
    let tessdata = [
        resource_dir.join("tessdata"),
        resource_dir.join("resources/tessdata"),
    ]
    .into_iter()
    .find(|dir| dir.join("eng.traineddata").is_file());
    if let Some(tessdata) = tessdata {
        command.env("CORUFRAME_TESSDATA_DIR", &tessdata);
        command.env("TESSDATA_PREFIX", &tessdata);
    }
    let bin = resource_dir.join("bin");
    if bin.is_dir() {
        let mut paths = vec![bin];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            command.env("PATH", joined);
        }
    }
    let libraries = resource_dir.join("lib");
    if libraries.is_dir() {
        let mut dirs = vec![libraries];
        if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
            dirs.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(dirs) {
            command.env("LD_LIBRARY_PATH", joined);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{gate, tail_path};

    #[test]
    fn the_toggle_refuses_a_machine_that_cannot_draw() {
        assert!(gate(Some(true), Some("5.4")).is_ok());
        assert!(gate(Some(false), Some("5.4"))
            .unwrap_err()
            .contains("layer-shell"));
        assert!(gate(None, Some("5.4"))
            .unwrap_err()
            .contains("Could not check"));
        assert!(gate(Some(true), None).unwrap_err().contains("Tesseract"));
    }

    #[test]
    fn a_log_tail_keeps_the_newest_lines_in_order() {
        let path = std::env::temp_dir().join(format!("coruframe-tail-{}", std::process::id()));
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert_eq!(tail_path(&path, 2).unwrap(), "two\nthree");
        assert_eq!(tail_path(&path, 0), None);
        let _ = std::fs::remove_file(path);
    }
}
