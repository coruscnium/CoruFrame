// EE.log's header carries a per-launch timestamp: the only identity stable
// between a live `watch` and a later `scan` of the same log, and therefore the
// ledger's dedupe key. The game truncates the file before writing that header,
// so the key has to be resolved lazily and never from an empty file.

pub const HEADER: &str = "Current time: ";
const HEADER_WINDOW: usize = 32;
const BUFFER_LIMIT: usize = 16;

pub fn from_text(text: &str) -> Option<String> {
    text.lines()
        .take(HEADER_WINDOW)
        .find_map(from_line)
        .or_else(|| {
            text.lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
        })
}

pub fn from_line(line: &str) -> Option<String> {
    line.split_once(HEADER)
        .map(|(_, rest)| rest.trim().to_string())
        .filter(|rest| !rest.is_empty())
}

#[derive(Debug)]
pub struct Tracker {
    key: Option<String>,
    pending: Vec<String>,
}

impl Tracker {
    pub fn new(key: Option<String>) -> Self {
        Self {
            key,
            pending: Vec::new(),
        }
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    // Returns the lines that are now safe to process; empty while the key is unknown.
    pub fn push(&mut self, line: String) -> Vec<String> {
        if self.key.is_some() {
            return vec![line];
        }
        self.pending.push(line);
        if let Some(key) = self.pending.last().and_then(|line| from_line(line)) {
            self.key = Some(key);
            return std::mem::take(&mut self.pending);
        }
        if self.pending.len() >= BUFFER_LIMIT {
            // Header absent or moved: fall back to the first non-empty line, matching `from_text`.
            self.key = self
                .pending
                .iter()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string());
            return std::mem::take(&mut self.pending);
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_header_timestamp() {
        let text = "0.067 Sys [Diag]: Windows user-name: steamuser\n\
                    0.068 Sys [Diag]: Current time: Fri Oct 23 03:01:32 2026 [UTC: Fri Oct 23 02:01:32 2026]\n";
        assert_eq!(
            from_text(text).as_deref(),
            Some("Fri Oct 23 03:01:32 2026 [UTC: Fri Oct 23 02:01:32 2026]")
        );
    }

    #[test]
    fn falls_back_to_the_first_line() {
        assert_eq!(from_text("\nfirst\nsecond\n").as_deref(), Some("first"));
    }

    #[test]
    fn resolves_on_the_header_and_releases_buffered_lines() {
        let mut tracker = Tracker::new(None);
        assert!(tracker.push("line one".into()).is_empty());
        let ready = tracker.push("0.068 Sys [Diag]: Current time: stamp".into());
        assert_eq!(ready, ["line one", "0.068 Sys [Diag]: Current time: stamp"]);
        assert_eq!(tracker.key(), Some("stamp"));
        assert_eq!(tracker.push("after".into()), ["after"]);
    }

    #[test]
    fn falls_back_after_the_buffer_limit() {
        let mut tracker = Tracker::new(None);
        let mut ready = Vec::new();
        for index in 0..BUFFER_LIMIT {
            ready = tracker.push(format!("line {index}"));
        }
        assert_eq!(ready.len(), BUFFER_LIMIT);
        assert_eq!(tracker.key(), Some("line 0"));
    }
}
