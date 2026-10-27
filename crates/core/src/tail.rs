use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const TAIL_WINDOW: usize = 32;

pub struct Follower {
    path: PathBuf,
    pos: u64,
    partial: Vec<u8>,
    truncated: bool,
    // Last bytes before `pos`, so a truncate-and-regrow that outruns the old size is still caught.
    tail: Vec<u8>,
}

impl Follower {
    pub fn new(path: impl Into<PathBuf>, from_start: bool) -> Self {
        let path = path.into();
        let pos = if from_start {
            0
        } else {
            last_line_start(&path).unwrap_or(0)
        };
        let tail = read_tail(&path, pos).unwrap_or_default();
        Self {
            path,
            pos,
            partial: Vec::new(),
            truncated: false,
            tail,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    // A truncation means a new game launch: the caller's parser state and session key are stale.
    pub fn last_poll_truncated(&self) -> bool {
        self.truncated
    }

    // Warframe truncates EE.log on launch instead of rotating it, so a shorter file means "start over".
    pub fn poll(&mut self) -> std::io::Result<Vec<String>> {
        let mut file = File::open(&self.path)?;
        let len = file.metadata()?.len();
        self.truncated = len < self.pos || self.window_changed(&mut file)?;
        if self.truncated {
            self.pos = 0;
            self.partial.clear();
            self.tail.clear();
        }
        file.seek(SeekFrom::Start(self.pos))?;
        let mut chunk = Vec::new();
        file.read_to_end(&mut chunk)?;
        self.pos += chunk.len() as u64;
        self.partial.extend_from_slice(&chunk);

        if !chunk.is_empty() {
            let mut tail = std::mem::take(&mut self.tail);
            tail.extend_from_slice(&chunk);
            let cut = tail.len().saturating_sub(TAIL_WINDOW);
            self.tail = tail.split_off(cut);
        }

        let mut lines = Vec::new();
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = self.partial.drain(..=end).collect();
            let text = String::from_utf8_lossy(&raw[..raw.len() - 1]);
            lines.push(text.trim_end_matches('\r').to_string());
        }
        Ok(lines)
    }

    fn window_changed(&self, file: &mut File) -> std::io::Result<bool> {
        if self.tail.is_empty() {
            return Ok(false);
        }
        let start = self.pos - self.tail.len() as u64;
        file.seek(SeekFrom::Start(start))?;
        let mut window = vec![0; self.tail.len()];
        file.read_exact(&mut window)?;
        Ok(window != self.tail)
    }
}

// Starting mid-line would emit a fragment; rewind to the last newline and let the line complete.
fn last_line_start(path: &Path) -> std::io::Result<u64> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let window = len.min(4096);
    file.seek(SeekFrom::Start(len - window))?;
    let mut bytes = vec![0; window as usize];
    file.read_exact(&mut bytes)?;
    Ok(match bytes.iter().rposition(|b| *b == b'\n') {
        Some(index) => len - window + index as u64 + 1,
        None => 0,
    })
}

fn read_tail(path: &Path, pos: u64) -> std::io::Result<Vec<u8>> {
    let start = pos.saturating_sub(TAIL_WINDOW as u64);
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start))?;
    let mut tail = vec![0; (pos - start) as usize];
    file.read_exact(&mut tail)?;
    Ok(tail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn survives_truncation_and_partial_lines() {
        let dir = std::env::temp_dir().join(format!("coruframe-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("EE.log");
        std::fs::write(&path, "one\ntw").unwrap();

        let mut f = Follower::new(&path, true);
        assert_eq!(f.poll().unwrap(), vec!["one"]);
        assert!(f.poll().unwrap().is_empty());

        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"o\n").unwrap();
        assert_eq!(f.poll().unwrap(), vec!["two"]);
        assert!(!f.last_poll_truncated());

        // Rewritten and already longer than the old offset: size alone cannot catch this.
        std::fs::write(&path, "reset\n".repeat(20)).unwrap();
        assert_eq!(f.poll().unwrap(), vec!["reset"; 20]);
        assert!(f.last_poll_truncated());

        std::fs::write(&path, "x\n").unwrap();
        assert_eq!(f.poll().unwrap(), vec!["x"]);
        assert!(f.last_poll_truncated());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_end_finishes_a_partial_line() {
        let dir =
            std::env::temp_dir().join(format!("coruframe-tail-partial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("EE.log");
        std::fs::write(&path, "one\ntw").unwrap();

        let mut f = Follower::new(&path, false);
        assert!(f.poll().unwrap().is_empty());
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"o\n")
            .unwrap();
        assert_eq!(f.poll().unwrap(), vec!["two"]);
        assert!(!f.last_poll_truncated());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn from_end_catches_a_rewrite() {
        let dir = std::env::temp_dir().join(format!("coruframe-tail-end-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("EE.log");
        std::fs::write(&path, "old\ndata\n").unwrap();

        let mut f = Follower::new(&path, false);
        assert!(f.poll().unwrap().is_empty());

        std::fs::write(&path, "new session\nthat is longer\n").unwrap();
        assert_eq!(f.poll().unwrap(), vec!["new session", "that is longer"]);
        assert!(f.last_poll_truncated());

        std::fs::remove_dir_all(&dir).ok();
    }
}
