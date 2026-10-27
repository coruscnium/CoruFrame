use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::time::Duration;

use coruframe_core::ocr::{parse_tsv, Ocr};
use wait_timeout::ChildExt;

pub const OCR_TIMEOUT: Duration = Duration::from_secs(8);

static USER_WORDS: OnceLock<Option<PathBuf>> = OnceLock::new();

// Generated once per process from the catalog/itemdb/relics on disk. Tests and
// the eval command can leave it unset; tesseract then uses its default model.
pub fn set_user_words(path: Option<PathBuf>) {
    let _ = USER_WORDS.set(path);
}

fn user_words() -> Option<&'static Path> {
    USER_WORDS.get().and_then(|path| path.as_deref())
}

// Status only: no stdout pipe can fill and block the child. The caller decides
// whether stderr is inherited.
pub fn run_timed_status(command: &mut Command, timeout: Duration) -> Result<ExitStatus, String> {
    command.stdout(Stdio::null());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    match child.wait_timeout(timeout) {
        Ok(Some(status)) => Ok(status),
        Ok(None) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(format!("timed out after {timeout:?}"))
        }
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error.to_string())
        }
    }
}

pub fn available() -> bool {
    Command::new("tesseract")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

// The reward pipeline hard-codes `-l eng`; a tesseract without the English
// model is as broken as a missing binary, so the doctor distinguishes them.
// A bundled model is checked directly: `--list-langs` resolves the model
// through TESSDATA_PREFIX, whose spelling differs between Tesseract builds.
pub fn eng_available() -> bool {
    if let Some(dir) = std::env::var_os("CORUFRAME_TESSDATA_DIR") {
        return Path::new(&dir).join("eng.traineddata").is_file();
    }
    Command::new("tesseract")
        .arg("--list-langs")
        .output()
        .is_ok_and(|output| {
            output.status.success()
                && String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .any(|line| line.trim() == "eng")
        })
}

// One engine, one invocation: the arguments that make the baseline measurable
// (OEM LSTM, 300 dpi after the upscale variants, the generated user words).
fn tesseract(input: &Path, base: &Path, psm: u32, extra: &[&str]) -> Result<(), String> {
    let psm = psm.to_string();
    let mut command = Command::new("tesseract");
    command
        .arg(input)
        .arg(base)
        .args(["-l", "eng", "--oem", "1", "--dpi", "300"])
        .args(["--psm", psm.as_str()]);
    if let Some(dir) = std::env::var_os("CORUFRAME_TESSDATA_DIR") {
        command.arg("--tessdata-dir").arg(dir);
    }
    if let Some(words) = user_words() {
        command.arg("--user-words").arg(words);
    }
    // Config file arguments must come last: everything after the first one is
    // read as another config file, so `tsv` before --user-words would parse the
    // word list as parameters.
    command.args(extra);
    // Tesseract chatters about every dictionary word it considers unknown
    // ("Parameter not found: Yareli"); the exit status is the useful signal.
    command.stderr(Stdio::null());
    let status = run_timed_status(&mut command, OCR_TIMEOUT)
        .map_err(|error| format!("tesseract: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("tesseract exited with {status}"))
    }
}

pub fn ocr_tsv(path: &Path, psm: u32) -> Result<Ocr, String> {
    let base = path.with_extension("ocr");
    let mut tsv = base.clone().into_os_string();
    tsv.push(".tsv");
    let tsv = PathBuf::from(tsv);
    let parsed = tesseract(path, &base, psm, &["tsv"]).and_then(|()| {
        std::fs::read_to_string(&tsv).map_err(|error| format!("read {}: {error}", tsv.display()))
    });
    let _ = std::fs::remove_file(&tsv);
    parsed.map(|source| parse_tsv(&source))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn times_out_a_slow_child() {
        let mut command = Command::new("sh");
        command.arg("-c").arg("sleep 5");
        let started = Instant::now();
        let error = run_timed_status(&mut command, Duration::from_millis(150)).unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(500));
        assert!(error.contains("timed out"), "{error}");
    }

    #[test]
    fn returns_a_quick_status() {
        let mut command = Command::new("true");
        assert!(run_timed_status(&mut command, Duration::from_secs(2))
            .unwrap()
            .success());
    }
}
