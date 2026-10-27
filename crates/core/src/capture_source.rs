use serde::{Deserialize, Serialize};

// What the portal returned for the chosen source; printed by `capture-source`
// and shown by the app. It never contains the restore token itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Selection {
    pub streams: Vec<SelectedStream>,
    pub token_stored: bool,
    pub restored: bool,
    pub start_ms: u64,
    pub frame: Option<CapturedFrameInfo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SelectedStream {
    pub source: String,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub node_id: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapturedFrameInfo {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: String,
    pub buffer: String,
    pub capture_ms: u64,
}
