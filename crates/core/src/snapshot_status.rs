use serde::{Deserialize, Serialize};

// Bumping the schema is how the app learns that a newer writer may have added
// arrays it does not know yet, and how the wrapper discards a cached fast-path
// working set whose regions predate the current required arrays. 3 moved the
// rank arrays into the required set.
pub const SNAPSHOT_SCHEMA: u64 = 3;
pub const STATUS_VERSION: u64 = 1;

pub fn path() -> std::path::PathBuf {
    crate::fetch::cache_dir().join("inventory-status.json")
}

// The app drops this file when the user asks for a manual scan. The wrapper
// consumes it and runs a full scan even when the fast path or settle would skip
// one.
pub fn scan_request_path() -> std::path::PathBuf {
    crate::fetch::cache_dir().join("inventory-scan-request")
}

// Small per-scan contract between the wrapper and the app. The app reloads the
// large raw file only when `content_hash` changes; the rest is display state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StatusFile {
    pub version: u64,
    pub snapshot_schema: u64,
    pub content_hash: String,
    pub captured_at: u64,
    pub verified_at: u64,
    pub last_scan_at: u64,
    // The live raw existed but could not be parsed. It is still overwritten by
    // the next complete scan; the app only needs to stop calling it empty.
    pub previous_unreadable: bool,
    // Persisted fast-path working set for the current game process. Seeded only
    // when `game_pid` still matches; a different PID discards it.
    pub game_pid: u32,
    pub fast_cluster: Option<(u64, u64)>,
    pub fast_regions: Vec<(u64, u64)>,
    pub last_scan: LastScan,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LastScan {
    pub complete: bool,
    pub limit: Option<String>,
    // A late scan-budget miss can accompany a complete, promoted capture; the
    // UI must not flag that as a failed scan.
    pub limit_blocking: bool,
    pub trigger: String,
    pub fast_path: String,
    pub bytes: u64,
    pub extraction_bytes: u64,
    pub regions: usize,
    pub duration_ms: u64,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_round_trips_and_reads_old_files() {
        let status = StatusFile {
            version: STATUS_VERSION,
            snapshot_schema: SNAPSHOT_SCHEMA,
            content_hash: "0123456789abcdef".to_string(),
            captured_at: 10,
            verified_at: 20,
            last_scan_at: 30,
            previous_unreadable: false,
            game_pid: 7,
            fast_cluster: Some((10, 20)),
            fast_regions: vec![(10, 30)],
            last_scan: LastScan {
                complete: true,
                limit: Some("scan_budget".to_string()),
                limit_blocking: false,
                trigger: "timer".to_string(),
                fast_path: "cluster".to_string(),
                bytes: 4 << 20,
                extraction_bytes: 4096,
                regions: 1,
                duration_ms: 7,
                error: None,
            },
        };
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(serde_json::from_str::<StatusFile>(&json).unwrap(), status);
    }

    #[test]
    fn a_minimal_status_file_fills_defaults() {
        let status: StatusFile = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert_eq!(status.version, 1);
        assert!(status.content_hash.is_empty());
        assert_eq!(status.game_pid, 0);
        assert!(status.fast_regions.is_empty());
        assert!(!status.last_scan.complete);
    }
}
