use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Read;
use std::os::unix::fs::FileExt;

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use coruframe_core::account;
use coruframe_core::fetch;
use coruframe_core::hash::{fnv1a_64, hex16};
use coruframe_core::inventory_keys;
use coruframe_core::logpath;
use coruframe_core::private_fs::{ensure_private_dir, write_private};
use coruframe_core::snapshot_status::{self, LastScan, StatusFile, SNAPSHOT_SCHEMA};
use coruframe_core::tail::Follower;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SCAN_CHUNK: usize = 1 << 20;
const SCAN_BUDGET: u64 = 4 << 30;
const MAX_HITS_PER_MARKER: usize = 24;
const CORE_HIT_CAP: usize = 256;
const UPGRADES_HIT_CAP: usize = 10_000;
const MAX_REPORT_SNIPPETS: usize = 24;
const MAX_REPORT_MARKERS: usize = 64;
const MAX_REPORT_OFFSETS: usize = 8;
const REPORT_INTERVAL: Duration = Duration::from_secs(60);
const SNIPPET_BEFORE: u64 = 512;
const SNIPPET_LEN: usize = 8192;
// A profile array is a JSON value in its own right; each one is read from its
// own hit. The shared budget is only a runaway guard.
const EXTRACTION_BUDGET: u64 = 768 << 20;
const EXTRACTION_WINDOW: u64 = 8 << 20;
// The game holds the profile JSON only while it syncs. Retry quickly after a
// miss so a short burst is not lost; back off once a complete snapshot lands.
const INCOMPLETE_RETRY: std::time::Duration = std::time::Duration::from_secs(5);
const FULL_FALLBACK: std::time::Duration = std::time::Duration::from_secs(60);
const BURST: std::time::Duration = std::time::Duration::from_secs(30);
const BURST_INTERVAL: std::time::Duration = std::time::Duration::from_secs(2);
// A timer-driven fast-path miss must not pay for a full 1.8 GB read; retry a
// full scan every fifth timer instead of every tick while the JSON is freed.
const FAST_MISSES_BEFORE_FULL: u32 = 5;
// A full disk or replaced cache directory fails every two-second status write;
// retry the small file at a human pace instead of spamming the log.
const WRITE_BACKOFF: Duration = Duration::from_secs(30);
// A partial generation that carries fewer tracked keys than this cannot tell
// "never owned" from "not synced yet"; refusing to synthesize avoids a false
// zero on first run. Revisit if a real low-collection account never promotes.
const MIN_CLUSTER_TRACKED: usize = 12;
const LOG_TRIGGERS: [&str; 7] = [
    "Logged in ",
    "MainMenu::LoginDone",
    "OnLoginDone",
    "CompleteLoginTasks",
    "Host_StartMatch",
    "Starting new background region",
    "SetSquadHostHubLevelName",
];
const MODULAR_XP: [&str; 8] = [
    "InfKitGun",
    "SUModular",
    "OperatorAmplifier",
    "ModularMelee",
    "/Tip/",
    "/Tips/",
    "/Balance/",
    "/Handle/",
];

// A snapshot is written live only when every tracked inventory array was
// captured. Category keys are usually present as empty arrays; treating them as
// optional would let one section silently disappear from the app. Ranks are
// required on the same terms but are never synthesized or carried.
const REQUIRED_KEYS: [&str; 6] = inventory_keys::CORE;
const OPTIONAL_KEYS: [&str; 8] = inventory_keys::CATEGORY;

// Core keys first so a fragment still reports the core state before optional
// sections; the complete-document check is what gates the write.
fn extract_keys() -> impl Iterator<Item = &'static str> {
    inventory_keys::TRACKED.iter().copied()
}

fn marker_hit_cap(name: &str) -> usize {
    if name == "Upgrades" {
        UPGRADES_HIT_CAP
    } else if is_inventory_key(name) {
        // The authoritative document may be later than the first copies.
        CORE_HIT_CAP
    } else {
        MAX_HITS_PER_MARKER
    }
}

#[derive(Debug, Serialize)]
struct MarkerHit {
    name: String,
    encoding: &'static str,
    offset: u64,
    snippet: String,
}

#[derive(Debug, Serialize)]
struct KeyHits {
    name: String,
    hits: usize,
    utf8: usize,
    utf16: usize,
    cap_reached: bool,
    offsets: Vec<u64>,
}

#[derive(Debug, Serialize)]
struct ExtractionUse {
    name: String,
    bytes: u64,
}

#[derive(Debug, Serialize)]
struct ScanReport {
    pid: u32,
    game: String,
    scanned_at: u64,
    captured_at: u64,
    duration_ms: u64,
    regions: usize,
    bytes: u64,
    extraction_bytes: u64,
    skipped_regions: usize,
    skipped_bytes: u64,
    truncated: bool,
    complete: bool,
    captured: Vec<String>,
    missing: Vec<String>,
    limit: Option<Limit>,
    trigger: String,
    fast_path: FastPath,
    cluster: Option<ClusterBounds>,
    profile_regions: Vec<(u64, u64)>,
    hits_total: usize,
    markers: Vec<MarkerHit>,
    key_hits: Vec<KeyHits>,
    extraction: Vec<ExtractionUse>,
    error: Option<String>,
}

// The report serializes the tier as a string, not a bool: the app and the docs
// need to tell "cluster" (4 MB) from "region" (330 MB).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum FastPath {
    None,
    Cluster,
    Region,
}

impl FastPath {
    fn as_str(self) -> &'static str {
        match self {
            FastPath::None => "none",
            FastPath::Cluster => "cluster",
            FastPath::Region => "region",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct ClusterBounds {
    start: u64,
    end: u64,
}

// A limit carries both the reason shown in diagnostics and whether it may stop
// a complete generation from being promoted. Diagnostics never block; a late
// scan-budget miss after every tracked array was captured does not either.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Limit {
    code: String,
    blocking: bool,
}

impl Limit {
    fn new(code: impl Into<String>, blocking: bool) -> Self {
        Self {
            code: code.into(),
            blocking,
        }
    }

    fn blocking(code: impl Into<String>) -> Self {
        Self::new(code, true)
    }

    fn is_blocking(&self) -> bool {
        self.blocking
    }
}

impl Serialize for Limit {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.code)
    }
}

// A partial scan must not shrink the persisted working set; keep the last
// complete cache for the same PID. A cache written under an older schema can
// miss arrays that are required now, so it is dropped instead of carried.
fn status_cache(
    promoted: bool,
    cluster: Option<(u64, u64)>,
    regions: &[(u64, u64)],
    previous: Option<&StatusFile>,
    pid: u32,
) -> FastCache {
    let fresh = FastCache {
        cluster,
        region: regions.to_vec(),
    };
    if promoted {
        return fresh;
    }
    match previous.filter(|status| status.game_pid == pid) {
        Some(status) if status.snapshot_schema != SNAPSHOT_SCHEMA => FastCache::default(),
        Some(status) => FastCache {
            cluster: status.fast_cluster,
            region: status.fast_regions.clone(),
        },
        None => fresh,
    }
}

fn promotable(complete: bool, limit: Option<&Limit>, all_tracked_in_cluster: bool) -> bool {
    if !complete {
        return false;
    }
    match limit {
        None => true,
        Some(limit) => !limit.is_blocking() && all_tracked_in_cluster,
    }
}

struct ScanOutcome {
    report: ScanReport,
    status: StatusFile,
    promoted: bool,
}

struct Marker {
    name: &'static str,
    bytes: Vec<u8>,
    utf16: bool,
}

fn marker(name: &'static str) -> Vec<Marker> {
    let utf8 = format!("\"{name}\"").into_bytes();
    let mut utf16 = Vec::with_capacity(utf8.len() * 2);
    for byte in &utf8 {
        utf16.push(*byte);
        utf16.push(0);
    }
    vec![
        Marker {
            name,
            bytes: utf8,
            utf16: false,
        },
        Marker {
            name,
            bytes: utf16,
            utf16: true,
        },
    ]
}

fn markers() -> Vec<Marker> {
    [
        "ItemCount",
        "ItemType",
        "icbdata",
        "Suits",
        "Sentinels",
        "KubrowPets",
        "MechSuits",
        "SpaceSuits",
        "SpaceGuns",
        "SpaceMelee",
        "LongGuns",
        "Pistols",
        "Melee",
        "SentinelWeapons",
        "OperatorAmps",
        "MiscItems",
        "RawUpgrades",
        "Upgrades",
        // Profile mastery history; optional because the shape is build-specific.
        "XPInfo",
    ]
    .into_iter()
    .flat_map(marker)
    .collect()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn read_small(path: String) -> Option<String> {
    let mut text = String::new();
    OpenOptions::new()
        .read(true)
        .open(path)
        .ok()?
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

fn cmdline(pid: u32) -> String {
    let mut bytes = Vec::new();
    if let Ok(mut file) = OpenOptions::new()
        .read(true)
        .open(format!("/proc/{pid}/cmdline"))
    {
        let _ = file.read_to_end(&mut bytes);
    }
    bytes
        .split(|byte| *byte == 0)
        .map(|part| String::from_utf8_lossy(part))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn parent_of(pid: u32) -> Option<u32> {
    let status = read_small(format!("/proc/{pid}/status"))?;
    status.lines().find_map(|line| {
        line.strip_prefix("PPid:")
            .and_then(|value| value.trim().parse().ok())
    })
}

fn ancestor_depth(mut pid: u32, ancestor: u32) -> usize {
    for depth in 0..64 {
        if pid == ancestor {
            return depth;
        }
        let parent = match parent_of(pid) {
            Some(parent) if parent > 0 && parent != pid => parent,
            _ => return 0,
        };
        pid = parent;
    }
    0
}

fn find_game_pid(wrapper: u32) -> Option<u32> {
    let mut fallback: Option<(u32, usize)> = None;
    for entry in fs::read_dir("/proc").ok()? {
        // A process can exit between readdir and stat; one vanished PID must
        // not abort the whole search.
        let Ok(entry) = entry else { continue };
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == wrapper {
            continue;
        }
        let comm = read_small(format!("/proc/{pid}/comm"))
            .unwrap_or_default()
            .to_lowercase();
        let args = cmdline(pid);
        let named_game = comm.contains("warframe");
        let command_game = args.contains("warframe.x64.exe") || args.contains("warframe.exe");
        if !named_game && !command_game {
            continue;
        }
        let depth = ancestor_depth(pid, wrapper);
        if depth == 0 {
            continue;
        }
        if named_game {
            return Some(pid);
        }
        // Launchers such as `env` or a Steam prefix also carry the game path in
        // their cmdline; the actual game is the deepest matching descendant.
        if fallback.is_none_or(|(_, best)| depth > best) {
            fallback = Some((pid, depth));
        }
    }
    fallback.map(|(pid, _)| pid)
}

fn is_candidate_region(path: &str) -> bool {
    // Anonymous memory and explicit shared buffers can hold the profile;
    // normal file-backed assets, vdso and the stack cannot.
    let file_backed =
        path.starts_with('/') && !path.starts_with("/memfd:") && !path.starts_with("/dev/shm/");
    !(file_backed || path.starts_with("[v") || path.starts_with("[stack]"))
}

fn parse_map(line: &str) -> Option<(u64, u64, bool, String)> {
    let mut fields = line.split_whitespace();
    let range = fields.next()?;
    let permissions = fields.next()?;
    let _offset = fields.next()?;
    let _device = fields.next()?;
    let _inode = fields.next()?;
    let path = fields.collect::<Vec<_>>().join(" ");
    let (start, end) = range.split_once('-')?;
    Some((
        u64::from_str_radix(start, 16).ok()?,
        u64::from_str_radix(end, 16).ok()?,
        permissions.starts_with('r') && permissions.contains('w'),
        path,
    ))
}

fn is_sensitive(key: &str) -> bool {
    const SENSITIVE: [&str; 11] = [
        "token",
        "secret",
        "password",
        "email",
        "steamid",
        "twitchid",
        "accountid",
        "nonce",
        "session",
        "auth",
        "key",
    ];
    let lower = key.to_ascii_lowercase();
    lower == "id" || SENSITIVE.iter().any(|word| lower.contains(word))
}

// End of one JSON value. Accounts carry numbers and arrays next to strings, and
// the sensitive ones are replaced whole instead of leaking a numeric id.
fn value_end(bytes: &[u8], start: usize) -> usize {
    let Some(first) = bytes.get(start) else {
        return start;
    };
    if *first == b'"' {
        let mut index = start + 1;
        while index < bytes.len() {
            if bytes[index] == b'\\' {
                index += 2;
                continue;
            }
            if bytes[index] == b'"' {
                return index + 1;
            }
            index += 1;
        }
        return bytes.len();
    }
    if *first == b'[' || *first == b'{' {
        let mut depth = 0isize;
        let mut in_string = false;
        let mut escaped = false;
        for (index, byte) in bytes.iter().enumerate().skip(start) {
            if in_string {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *byte {
                b'"' => in_string = true,
                b'[' | b'{' => depth += 1,
                b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
        }
        return bytes.len();
    }
    let mut index = start;
    while index < bytes.len() && !matches!(bytes[index], b',' | b'}' | b']') {
        index += 1;
    }
    index
}

fn contains_redacted(bytes: &[u8]) -> bool {
    bytes
        .windows(b"[redacted]".len())
        .any(|window| window == b"[redacted]")
}

// Keyed JSON values cover the account document; bare values still leak when a
// marker lands outside an object (for example a token in a config blob).
pub(crate) fn redact(text: &str) -> String {
    redact_bare(&redact_json(text))
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'=' | b'+' | b'_' | b'-' | b'.')
}

fn email_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric()
            || matches!(bytes[index], b'.' | b'_' | b'%' | b'+' | b'-'))
    {
        index += 1;
    }
    if index == start || bytes.get(index) != Some(&b'@') {
        return None;
    }
    index += 1;
    let domain_start = index;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'.' | b'-'))
    {
        index += 1;
    }
    let domain = &bytes[domain_start..index];
    let dot = domain.iter().rposition(|byte| *byte == b'.')?;
    let tld = &domain[dot + 1..];
    (!tld.is_empty() && tld.len() >= 2 && tld.iter().all(u8::is_ascii_alphabetic)).then_some(index)
}

pub(crate) fn redact_bare(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if let Some(end) = email_end(bytes, index) {
            out.push_str("[redacted]");
            index = end;
            continue;
        }
        if is_token_byte(bytes[index]) {
            let mut end = index;
            while end < bytes.len() && is_token_byte(bytes[end]) {
                end += 1;
            }
            let run = &bytes[index..end];
            // A path or a sentence has separators inside 32 bytes; a bare token
            // does not, and always mixes letters with digits.
            if run.len() >= 32 && run.iter().any(u8::is_ascii_digit) {
                out.push_str("[redacted]");
            } else {
                out.push_str(&text[index..end]);
            }
            index = end;
            continue;
        }
        let character = text[index..].chars().next().unwrap();
        out.push(character);
        index += character.len_utf8();
    }
    out
}

fn redact_json(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let key_start = index + 1;
            let mut key_end = key_start;
            while key_end < bytes.len() && bytes[key_end] != b'"' {
                key_end += 1;
            }
            if key_end < bytes.len() {
                let key = &text[key_start..key_end];
                let mut colon = key_end + 1;
                while colon < bytes.len() && bytes[colon].is_ascii_whitespace() {
                    colon += 1;
                }
                if colon < bytes.len() && bytes[colon] == b':' {
                    let mut value_start = colon + 1;
                    while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                        value_start += 1;
                    }
                    let end = value_end(bytes, value_start);
                    out.push_str(&text[index..=key_end]);
                    out.push(':');
                    if is_sensitive(key) {
                        out.push_str("\"[redacted]\"");
                    } else {
                        let value = &text[value_start..end];
                        // A sensitive key nested in an object or array is only
                        // visible to the inner parse.
                        if value.starts_with(['{', '[']) {
                            out.push_str(&redact_json(value));
                        } else {
                            out.push_str(value);
                        }
                    }
                    index = end;
                    continue;
                }
                out.push_str(&text[index..=key_end]);
                index = key_end + 1;
                continue;
            }
        }
        let character = text[index..].chars().next().unwrap();
        out.push(character);
        index += character.len_utf8();
    }
    out
}

fn decode_snippet(bytes: &[u8], utf16: bool) -> String {
    let text = if utf16 {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };
    let cleaned = text.replace('\0', " ");
    redact(&cleaned).chars().take(SNIPPET_LEN).collect()
}

#[allow(clippy::too_many_arguments)]
fn record_hit(
    mem: &std::fs::File,
    start: u64,
    end: u64,
    absolute: u64,
    definition: &Marker,
    seen: &mut HashMap<(String, bool), usize>,
    hits: &mut Vec<MarkerHit>,
) {
    let seen_key = (definition.name.to_string(), definition.utf16);
    let count = seen.get(&seen_key).copied().unwrap_or(0);
    // The report is a wrapper-only diagnostic; snippets are opt-in so the file
    // does not carry account data by default.
    let debug = std::env::var_os("CORUFRAME_SNAPSHOT_DEBUG").is_some();
    let snippet = if debug && count < MAX_REPORT_SNIPPETS {
        // Markers often sit at the start of a heap region; never read before
        // the region, or the snippet is lost to EIO.
        let snippet_start = absolute.saturating_sub(SNIPPET_BEFORE).max(start);
        let snippet_length = ((end - snippet_start) as usize).min(SNIPPET_LEN);
        let mut snippet = vec![0u8; snippet_length];
        let snippet_read = mem.read_at(&mut snippet, snippet_start).unwrap_or(0);
        decode_snippet(&snippet[..snippet_read], definition.utf16)
    } else {
        String::new()
    };
    if debug {
        eprintln!(
            "snapshot: hit {} ({}) at {absolute:x}",
            definition.name,
            if definition.utf16 {
                "utf-16le"
            } else {
                "utf-8"
            }
        );
    }
    hits.push(MarkerHit {
        name: definition.name.to_string(),
        encoding: if definition.utf16 {
            "utf-16le"
        } else {
            "utf-8"
        },
        offset: absolute,
        snippet,
    });
    *seen.entry(seen_key).or_default() += 1;
}

#[allow(clippy::too_many_arguments)]
fn scan_region(
    mem: &std::fs::File,
    start: u64,
    end: u64,
    definitions: &[Marker],
    hits: &mut Vec<MarkerHit>,
    seen: &mut HashMap<(String, bool), usize>,
    buffer: &mut Vec<u8>,
    tail: &mut Vec<u8>,
    state: &mut (u64, usize, bool),
) {
    let max_marker = definitions
        .iter()
        .map(|marker| marker.bytes.len())
        .max()
        .unwrap_or(1);
    // Every marker starts with a quote, so one memchr pass finds every
    // candidate. The byte after the quote narrows it to one or two names;
    // scanning each marker separately over the whole window was ~50x slower.
    let mut candidates: [Vec<&Marker>; 256] = std::array::from_fn(|_| Vec::new());
    for definition in definitions {
        if let Some(second) = definition.bytes.get(1) {
            candidates[*second as usize].push(definition);
        }
    }
    let mut offset = start;
    while offset < end {
        if state.2 {
            return;
        }
        let length = (end - offset).min(SCAN_CHUNK as u64) as usize;
        buffer.resize(length, 0);
        let read = match mem.read_at(buffer, offset) {
            Ok(read) => read,
            Err(_) => {
                offset += length as u64;
                continue;
            }
        };
        if read == 0 {
            offset += length as u64;
            continue;
        }
        state.0 += read as u64;
        state.1 += 1;
        if state.0 >= SCAN_BUDGET {
            state.2 = true;
        }

        let mut window = Vec::with_capacity(tail.len() + read);
        window.extend_from_slice(tail);
        window.extend_from_slice(&buffer[..read]);
        let base = offset.saturating_sub(tail.len() as u64);

        let any_open = definitions.iter().any(|definition| {
            seen.get(&(definition.name.to_string(), definition.utf16))
                .copied()
                .unwrap_or(0)
                < marker_hit_cap(definition.name)
        });
        if any_open {
            let mut from = 0;
            while from < window.len() {
                let Some(found) = memchr::memchr(b'"', &window[from..]) else {
                    break;
                };
                let index = from + found;
                let Some(second) = window.get(index + 1).copied() else {
                    break;
                };
                for definition in &candidates[second as usize] {
                    if !window[index..].starts_with(&definition.bytes) {
                        continue;
                    }
                    if seen
                        .get(&(definition.name.to_string(), definition.utf16))
                        .copied()
                        .unwrap_or(0)
                        >= marker_hit_cap(definition.name)
                    {
                        continue;
                    }
                    record_hit(mem, start, end, base + index as u64, definition, seen, hits);
                }
                from = index + 1;
            }
        }

        if window.len() >= max_marker {
            *tail = window[window.len() - (max_marker - 1)..].to_vec();
        } else {
            *tail = window;
        }
        offset += read as u64;
        if read < length {
            offset += (length - read) as u64;
        }
    }
}

fn balanced_array(window: &[u8], start: usize) -> Option<&[u8]> {
    let mut depth = 0isize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, byte) in window.iter().enumerate().skip(start) {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match *byte {
            b'"' => in_string = true,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&window[start..=index]);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= haystack.len() {
        return None;
    }
    haystack[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|position| from + position)
}

fn is_item_array(value: &serde_json::Value) -> bool {
    value
        .as_array()
        .and_then(|items| items.first())
        .is_some_and(|first| first.get("ItemType").is_some())
}

fn is_inventory_key(key: &str) -> bool {
    inventory_keys::is_inventory(key)
}

fn object_array_start(mem: &std::fs::File, offset: u64, end: u64, allow_empty: bool) -> bool {
    let length = end.saturating_sub(offset).min(512) as usize;
    if length == 0 {
        return false;
    }
    let mut buffer = vec![0u8; length];
    let read = mem.read_at(&mut buffer, offset).unwrap_or(0);
    let bytes = &buffer[..read];
    let Some(colon) = find_bytes(bytes, b":", 0) else {
        return false;
    };
    let Some(bracket) = find_bytes(bytes, b"[", colon) else {
        return false;
    };
    bytes[bracket + 1..]
        .iter()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b'{' || (allow_empty && *byte == b']'))
}

// A missing value and an exhausted budget are different failures: the first is
// normal on a partial generation, the second is a limit worth reporting.
#[derive(Debug, PartialEq)]
enum ExtractMiss {
    Unavailable,
    Budget,
    TooLarge,
}

// Parse one inventory array from its own hit. Arrays can be megabytes apart and
// a fragment of the account document is still valid JSON on its own.
fn extract_array(
    mem: &std::fs::File,
    region_start: u64,
    region_end: u64,
    marker: u64,
    key: &str,
    budget: &mut u64,
) -> std::result::Result<serde_json::Value, ExtractMiss> {
    let start = marker.saturating_sub(1024).max(region_start);
    let end = marker.saturating_add(EXTRACTION_WINDOW).min(region_end);
    let window_full = end.saturating_sub(start) >= EXTRACTION_WINDOW;
    let mut buffer = Vec::new();
    let mut offset = start;
    while offset < end {
        if *budget == 0 {
            return Err(ExtractMiss::Budget);
        }
        let length = (end - offset).min(SCAN_CHUNK as u64).min(*budget) as usize;
        let mut chunk = vec![0u8; length];
        let read = mem.read_at(&mut chunk, offset).unwrap_or(0);
        if read == 0 {
            break;
        }
        *budget -= read as u64;
        buffer.extend_from_slice(&chunk[..read]);
        offset += read as u64;
    }
    let needle = format!("\"{key}\"");
    let local = (marker - start) as usize;
    let position = find_bytes(&buffer, needle.as_bytes(), local).ok_or(ExtractMiss::Unavailable)?;
    let mut cursor = position + needle.len();
    while cursor < buffer.len() && buffer[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor >= buffer.len() || buffer[cursor] != b':' {
        return Err(ExtractMiss::Unavailable);
    }
    cursor += 1;
    while cursor < buffer.len() && buffer[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor >= buffer.len() || buffer[cursor] != b'[' {
        return Err(ExtractMiss::Unavailable);
    }
    let array = match balanced_array(&buffer, cursor) {
        Some(array) => array,
        // A full window with no closing bracket means the array is larger than
        // the per-hit window, not that the copy is missing.
        None => {
            return Err(if window_full {
                ExtractMiss::TooLarge
            } else {
                ExtractMiss::Unavailable
            })
        }
    };
    let value =
        serde_json::from_slice::<serde_json::Value>(array).map_err(|_| ExtractMiss::Unavailable)?;
    value
        .is_array()
        .then_some(value)
        .ok_or(ExtractMiss::Unavailable)
}

// Copies of an array follow each other in memory; the first complete copy is
// the same value, so stop there instead of paying for every duplicate.
// Copies are not versions: a full sync writes all core keys together, so the
// cluster around a MiscItems hit is the generation to prefer. A key missing from
// that cluster falls back to another copy and is flagged in the metadata.
fn cluster_span(hits: &[MarkerHit]) -> Option<(u64, u64)> {
    const CLUSTER: u64 = 2 << 20;
    let mut best: Option<(usize, u64)> = None;
    for center in hits
        .iter()
        .filter(|hit| hit.encoding == "utf-8" && is_inventory_key(&hit.name))
        .map(|hit| hit.offset)
    {
        let mut names = std::collections::HashSet::new();
        for hit in hits.iter().filter(|hit| {
            hit.encoding == "utf-8"
                && is_inventory_key(&hit.name)
                && hit.offset.abs_diff(center) <= CLUSTER
        }) {
            names.insert(hit.name.as_str());
        }
        let score = names.len();
        if best.is_none_or(|(best_score, _)| score > best_score) {
            best = Some((score, center));
        }
    }
    best.map(|(_, center)| {
        (
            center.saturating_sub(CLUSTER),
            center.saturating_add(CLUSTER),
        )
    })
}

fn cluster_score(hits: &[MarkerHit], cluster: Option<(u64, u64)>) -> usize {
    let Some((start, end)) = cluster else {
        return 0;
    };
    hits.iter()
        .filter(|hit| {
            hit.encoding == "utf-8"
                && inventory_keys::is_tracked(&hit.name)
                && start <= hit.offset
                && hit.offset < end
        })
        .map(|hit| hit.name.as_str())
        .collect::<std::collections::HashSet<_>>()
        .len()
}

// Requested bounds are virtual addresses and may be a 4 MiB cluster or a whole
// cached region. Clip each one to the live writable mappings: a mapping that
// moved or vanished contributes nothing, and a cluster reads only its own bytes.
fn selected_bounds(available: &[(u64, u64)], requested: Option<&[(u64, u64)]>) -> Vec<(u64, u64)> {
    let Some(requested) = requested else {
        return available.to_vec();
    };
    let mut selected: Vec<(u64, u64)> = Vec::new();
    for (requested_start, requested_end) in requested {
        for (start, end) in available {
            let start = (*requested_start).max(*start);
            let end = (*requested_end).min(*end);
            if start < end && !selected.contains(&(start, end)) {
                selected.push((start, end));
            }
        }
    }
    selected
}

fn extract_key(
    mem: &std::fs::File,
    region_bounds: &[(u64, u64)],
    hits: &[MarkerHit],
    key: &str,
    preferred: Option<(u64, u64)>,
    budget: &mut u64,
) -> std::result::Result<(serde_json::Value, u64), ExtractMiss> {
    let matching = || {
        hits.iter()
            .filter(|hit| hit.name == key && hit.encoding == "utf-8")
            .take(marker_hit_cap(key))
    };
    let allow_empty = is_inventory_key(key);
    let mut empty = None;
    let mut too_large = false;
    // Prefer the complete cluster, then any copy for a key that it lacks.
    for preferred_pass in [true, false] {
        for hit in matching() {
            if preferred.is_some_and(|(start, end)| start <= hit.offset && hit.offset < end)
                != preferred_pass
            {
                continue;
            }
            let Some((start, end)) = region_bounds
                .iter()
                .find(|(start, end)| *start <= hit.offset && hit.offset < *end)
            else {
                continue;
            };
            if !object_array_start(mem, hit.offset, *end, allow_empty) {
                continue;
            }
            let value = match extract_array(mem, *start, *end, hit.offset, key, budget) {
                Ok(value) => value,
                Err(ExtractMiss::Budget) => return Err(ExtractMiss::Budget),
                Err(ExtractMiss::TooLarge) => {
                    too_large = true;
                    continue;
                }
                Err(ExtractMiss::Unavailable) => continue,
            };
            if is_item_array(&value) {
                return Ok((value, hit.offset));
            }
            // An empty inventory array is a real "none owned", not a missing
            // section. Keep it only if no non-empty copy exists.
            if allow_empty && value.as_array().is_some_and(Vec::is_empty) {
                empty.get_or_insert((value, hit.offset));
            }
        }
    }
    empty.ok_or(if too_large {
        ExtractMiss::TooLarge
    } else {
        ExtractMiss::Unavailable
    })
}

// A corrupt live file is ignored for carry and does not become the empty
// inventory the app would otherwise show; the next complete scan replaces it.
fn previous_raw(bytes: Option<&[u8]>) -> (Option<serde_json::Value>, bool) {
    let value = bytes.and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok());
    // The live snapshot must be an object; valid JSON of another shape is just
    // as unusable as a parse failure.
    let unreadable =
        bytes.is_some() && value.as_ref().and_then(|value| value.as_object()).is_none();
    (value, unreadable)
}

// Never carry another game session's arrays into this one; an account switch
// or relaunch must not inherit the previous process's values. A snapshot
// without identity (legacy format) cannot prove the same login, so it carries
// nothing.
fn previous_arrays(
    raw: Option<&serde_json::Map<String, serde_json::Value>>,
    account: Option<&str>,
    pid: u32,
) -> serde_json::Map<String, serde_json::Value> {
    let Some(raw) = raw else {
        return serde_json::Map::new();
    };
    let meta = raw.get("_meta");
    let same_pid = meta
        .and_then(|meta| meta.get("game_pid"))
        .and_then(serde_json::Value::as_u64)
        == Some(pid as u64);
    let same_account = account.is_some()
        && meta
            .and_then(|meta| meta.get("account"))
            .and_then(serde_json::Value::as_str)
            == account;
    if same_pid && same_account {
        raw.clone()
    } else {
        serde_json::Map::new()
    }
}

// A category the player has never owned may not serialize at all. Treat a
// missing optional array as empty, or carry the last value if one exists, so a
// full snapshot is still possible. Carried values are flagged in the metadata,
// and synthesis is refused when the generation is too incomplete to tell
// "none owned" from "not synced yet".
fn fill_missing_optional(
    arrays: &mut serde_json::Map<String, serde_json::Value>,
    previous: &serde_json::Map<String, serde_json::Value>,
    hits: &[MarkerHit],
    allow_synthesis: bool,
    previous_at: Option<u64>,
) -> Vec<(String, &'static str, Option<u64>)> {
    let mut filled = Vec::new();
    for key in OPTIONAL_KEYS {
        if arrays.contains_key(key) {
            continue;
        }
        let present = previous.get(key).filter(|value| value.is_array());
        // A hit means the array exists but could not be parsed; never invent an
        // empty value over a present array. Only a key absent from the whole
        // serialization can safely mean "none owned".
        if present.is_none() && hits.iter().any(|hit| hit.name == *key) {
            continue;
        }
        let (value, source, observed_at) = match present {
            Some(value) => {
                // A carried array keeps the time it was actually read; the
                // snapshot mtime is only a fallback for legacy metadata.
                let observed = previous
                    .get("_meta")
                    .and_then(|meta| meta.get("arrays"))
                    .and_then(|arrays| arrays.get(key))
                    .and_then(|entry| entry.get("observed_at"))
                    .and_then(serde_json::Value::as_u64)
                    .or(previous_at);
                (value.clone(), "carried", observed)
            }
            // An absent category is not observed data. The snapshot's
            // captured_at is the honest timestamp, so the content hash stays
            // stable across scans instead of churning every time.
            None if allow_synthesis => (serde_json::Value::Array(Vec::new()), "synthesized", None),
            None => continue,
        };
        arrays.insert(key.to_string(), value);
        filled.push((key.to_string(), source, observed_at));
    }
    filled
}

fn snapshot_status(
    arrays: &serde_json::Map<String, serde_json::Value>,
) -> (bool, Vec<String>, Vec<String>) {
    // Ranks gate completeness but are deliberately excluded from the optional
    // fill: an absent rank array must fail the check, not look like "no mods".
    let gates = || {
        REQUIRED_KEYS
            .iter()
            .chain(OPTIONAL_KEYS.iter())
            .chain(inventory_keys::RANKS.iter())
    };
    let captured = extract_keys()
        .filter(|key| arrays.contains_key(*key))
        .map(|key| key.to_string())
        .collect();
    let missing = gates()
        .filter(|key| !arrays.contains_key(**key))
        .map(|key| key.to_string())
        .collect();
    let complete = gates().all(|key| arrays.contains_key(*key));
    (complete, captured, missing)
}

fn key_hits(hits: &[MarkerHit]) -> Vec<KeyHits> {
    extract_keys()
        .map(|name| {
            let matching: Vec<&MarkerHit> = hits.iter().filter(|hit| hit.name == name).collect();
            let utf8 = matching
                .iter()
                .filter(|hit| hit.encoding == "utf-8")
                .count();
            let utf16 = matching.len() - utf8;
            let cap = marker_hit_cap(name);
            KeyHits {
                name: name.to_string(),
                hits: matching.len(),
                utf8,
                utf16,
                cap_reached: utf8 >= cap || utf16 >= cap,
                offsets: matching.iter().map(|hit| hit.offset).collect(),
            }
        })
        .collect()
}

fn write_inventory(json: &str) -> std::result::Result<(), String> {
    let path = fetch::cache_dir().join("inventory-raw.json");
    write_private(&path, json.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))
}

fn write_status(status: &StatusFile) -> std::result::Result<(), String> {
    write_status_at(&snapshot_status::path(), status)
}

fn write_status_at(path: &std::path::Path, status: &StatusFile) -> std::result::Result<(), String> {
    let json = serde_json::to_vec(status).map_err(|error| format!("status json: {error}"))?;
    write_private(path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_status() -> Option<StatusFile> {
    let text = fs::read_to_string(snapshot_status::path()).ok()?;
    serde_json::from_str(&text).ok()
}

fn raw_mtime() -> Option<u64> {
    fs::metadata(fetch::cache_dir().join("inventory-raw.json"))
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|age| age.as_secs())
}

// The hash covers only what the user sees: the arrays, their provenance and the
// schema. Offsets, the cluster and scan timestamps are diagnostics and must not
// make an unchanged inventory look new.
fn content_hash(
    arrays: &serde_json::Map<String, serde_json::Value>,
    array_meta: &serde_json::Map<String, serde_json::Value>,
    schema: u64,
) -> String {
    let mut provenance = serde_json::Map::new();
    for (key, entry) in array_meta {
        let mut canonical = serde_json::Map::new();
        if let Some(source) = entry.get("source") {
            canonical.insert("source".to_string(), source.clone());
        }
        if let Some(observed_at) = entry.get("observed_at") {
            canonical.insert("observed_at".to_string(), observed_at.clone());
        }
        provenance.insert(key.clone(), serde_json::Value::Object(canonical));
    }
    let canonical = serde_json::json!({
        "schema": schema,
        "arrays": arrays,
        "provenance": provenance,
    });
    hex16(fnv1a_64(canonical.to_string().as_bytes()))
}

fn content_hash_of_raw(value: &serde_json::Value) -> Option<String> {
    let arrays = value.as_object()?;
    let schema = arrays
        .get("_meta")
        .and_then(|meta| meta.get("schema"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    let meta_arrays = arrays
        .get("_meta")
        .and_then(|meta| meta.get("arrays"))
        .and_then(serde_json::Value::as_object);
    let mut values = arrays.clone();
    values.remove("_meta");
    Some(content_hash(
        &values,
        meta_arrays.unwrap_or(&serde_json::Map::new()),
        schema,
    ))
}

// Never let a decoded profile leave the machine: the dump is limited to array
// lengths and the modular XP paths the slot mapping needs to diagnose.
fn write_profile_dump(arrays: &serde_json::Map<String, serde_json::Value>) {
    if std::env::var_os("CORUFRAME_SNAPSHOT_DUMP").is_none() {
        return;
    }
    let keys: serde_json::Map<String, serde_json::Value> = arrays
        .iter()
        .map(|(key, value)| {
            let length = value.as_array().map(Vec::len).unwrap_or(0);
            (key.clone(), serde_json::json!(length))
        })
        .collect();
    let mut modular_xp = Vec::new();
    if let Some(entries) = arrays.get("XPInfo").and_then(serde_json::Value::as_array) {
        for entry in entries {
            let Some(path) = entry.get("ItemType").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if !MODULAR_XP.iter().any(|part| path.contains(part)) {
                continue;
            }
            let xp = entry.get("XP").cloned().unwrap_or(serde_json::Value::Null);
            modular_xp.push(serde_json::json!({ "path": path, "xp": xp }));
        }
    }
    let document = serde_json::json!({
        "keys": keys,
        "modular_xp": modular_xp,
    });
    let text = redact(&document.to_string());
    let path = fetch::cache_dir().join("inventory-profile.keys.json");
    if let Err(error) = write_private(&path, text.as_bytes()) {
        eprintln!("snapshot: write {}: {error}", path.display());
    }
}

fn scan(
    pid: u32,
    account: Option<&str>,
    bounds: Option<&[(u64, u64)]>,
    trigger: &str,
    fast_path: FastPath,
) -> std::result::Result<ScanOutcome, String> {
    let started = Instant::now();
    let maps = read_small(format!("/proc/{pid}/maps"))
        .ok_or_else(|| format!("cannot read /proc/{pid}/maps"))?;
    let mem = OpenOptions::new()
        .read(true)
        .open(format!("/proc/{pid}/mem"))
        .map_err(|error| format!("open /proc/{pid}/mem: {error}"))?;

    let definitions = markers();
    let mut hits = Vec::new();
    let mut seen = HashMap::new();
    let mut buffer = Vec::with_capacity(SCAN_CHUNK);
    let mut tail = Vec::new();
    let mut state = (0u64, 0usize, false);
    let mut skipped_regions = 0usize;
    let mut skipped_bytes = 0u64;
    let mut available = Vec::new();
    for line in maps.lines() {
        let Some((start, end, writable, path)) = parse_map(line) else {
            continue;
        };
        if !writable || end <= start {
            continue;
        }
        // The synced inventory lives in an anonymous heap or a shared buffer;
        // skip file-backed assets and kernel objects so the scan stays bounded
        // and non-invasive. /memfd: and /dev/shm/ buffers are candidates.
        if !is_candidate_region(&path) {
            skipped_regions += 1;
            skipped_bytes += end - start;
            continue;
        }
        if std::env::var_os("CORUFRAME_SNAPSHOT_DEBUG").is_some() {
            eprintln!(
                "snapshot: region {start:x}-{end:x} size={} path={path:?}",
                end - start
            );
        }
        available.push((start, end));
    }
    let selected = selected_bounds(&available, bounds);

    for (start, end) in &selected {
        scan_region(
            &mem,
            *start,
            *end,
            &definitions,
            &mut hits,
            &mut seen,
            &mut buffer,
            &mut tail,
            &mut state,
        );
        if state.2 {
            break;
        }
    }

    let raw_path = fetch::cache_dir().join("inventory-raw.json");
    let previous_at = raw_mtime();
    // Each array is parsed as its own complete JSON value. The write gate below
    // requires every tracked array, so a partial read can never become a snapshot.
    let raw_bytes = fs::read(&raw_path).ok();
    let (raw_value, unreadable) = previous_raw(raw_bytes.as_deref());
    // A file that exists but cannot be read or parsed must be replaced; a
    // missing file is a normal first run.
    // A raw written by the old bare-token redactor still has [redacted] item
    // paths. Rewrite it once even when the tracked content hash is unchanged.
    let previous_redacted = raw_bytes.as_deref().is_some_and(contains_redacted);
    let previous_unreadable = raw_path.is_file() && (raw_bytes.is_none() || unreadable);
    let raw = raw_value
        .as_ref()
        .and_then(|value| value.as_object().cloned());
    let previous_status = read_status();
    let previous = previous_arrays(raw.as_ref(), account, pid);
    let scanned_at = now();
    let cluster = cluster_span(&hits);
    let allow_synthesis = cluster_score(&hits, cluster) >= MIN_CLUSTER_TRACKED;
    let mut arrays = serde_json::Map::new();
    let mut array_meta = serde_json::Map::new();
    let mut budget = EXTRACTION_BUDGET;
    let mut all_tracked_in_cluster = true;
    let mut limit: Option<Limit> = if state.2 {
        Some(Limit::blocking("scan_budget"))
    } else if fast_path != FastPath::None && selected.is_empty() {
        // The cached mapping was unmapped or moved; the caller falls back to a
        // wider tier or a full scan in this cycle.
        Some(Limit::blocking("cached_region_gone"))
    } else {
        None
    };
    let mut extraction_usage = Vec::new();
    for key in extract_keys() {
        let before = budget;
        let extracted = extract_key(&mem, &selected, &hits, key, cluster, &mut budget);
        extraction_usage.push(ExtractionUse {
            name: key.to_string(),
            bytes: before - budget,
        });
        let value = match extracted {
            Ok(value) => value,
            Err(ExtractMiss::Budget) => {
                // A diagnostic running out of budget cannot affect the
                // snapshot, so it must not block promotion.
                if inventory_keys::is_tracked(key) {
                    limit
                        .get_or_insert_with(|| Limit::blocking(format!("extraction_budget:{key}")));
                }
                continue;
            }
            Err(ExtractMiss::TooLarge) => {
                if inventory_keys::is_tracked(key) {
                    limit.get_or_insert_with(|| Limit::blocking(format!("array_too_large:{key}")));
                }
                continue;
            }
            Err(ExtractMiss::Unavailable) => continue,
        };
        let (value, offset) = value;
        let in_cluster = cluster.is_some_and(|(start, end)| start <= offset && offset < end);
        if inventory_keys::is_tracked(key) && !in_cluster {
            all_tracked_in_cluster = false;
        }
        array_meta.insert(
            key.to_string(),
            serde_json::json!({
                "offset": offset,
                "in_cluster": in_cluster,
                "count": value.as_array().map(Vec::len).unwrap_or(0),
                "source": "captured",
            }),
        );
        arrays.insert(key.to_string(), value);
    }
    for (key, source, observed_at) in
        fill_missing_optional(&mut arrays, &previous, &hits, allow_synthesis, previous_at)
    {
        let count = arrays
            .get(&key)
            .and_then(serde_json::Value::as_array)
            .map(Vec::len)
            .unwrap_or(0);
        let mut entry = serde_json::json!({
            "count": count,
            "source": source,
        });
        if let Some(observed_at) = observed_at {
            entry["observed_at"] = serde_json::json!(observed_at);
        }
        array_meta.insert(key, entry);
    }

    // The fast path works on full mappings, not on the bounds a tier happened to
    // clip, so a cluster-tier scan still caches the whole containing region.
    let mut profile_regions: Vec<(u64, u64)> = Vec::new();
    for (key, meta) in &array_meta {
        if !inventory_keys::is_tracked(key) {
            continue;
        }
        let Some(offset) = meta.get("offset").and_then(serde_json::Value::as_u64) else {
            continue;
        };
        if let Some(bounds) = available
            .iter()
            .find(|(start, end)| *start <= offset && offset < *end)
        {
            if !profile_regions.contains(bounds) {
                profile_regions.push(*bounds);
            }
        }
    }

    let (complete, captured, missing) = snapshot_status(&arrays);
    let hits_report = key_hits(&hits);
    if !complete {
        if !REQUIRED_KEYS.iter().any(|key| arrays.contains_key(*key)) {
            limit.get_or_insert_with(|| Limit::blocking("no_profile"));
        } else if !allow_synthesis {
            limit.get_or_insert_with(|| Limit::blocking("cluster_incomplete"));
        } else if let Some(capped) = hits_report.iter().find(|hits| {
            inventory_keys::is_tracked(&hits.name)
                && !arrays.contains_key(&hits.name)
                && hits.cap_reached
        }) {
            limit.get_or_insert_with(|| Limit::blocking(format!("hit_cap:{}", capped.name)));
        }
    }
    // A budget miss after every tracked array was already captured inside the
    // chosen cluster is a diagnostic, not a reason to discard a valid capture.
    if complete && all_tracked_in_cluster {
        if let Some(limit) = limit.as_mut() {
            if limit.code == "scan_budget" {
                limit.blocking = false;
            }
        }
    }
    let promote = promotable(complete, limit.as_ref(), all_tracked_in_cluster);
    if let Some(limit) = &limit {
        eprintln!("snapshot: limit {}", limit.code);
    }
    let candidate_hash = content_hash(&arrays, &array_meta, SNAPSHOT_SCHEMA);
    write_profile_dump(&arrays);
    let meta_limit = limit.as_ref().map(|limit| limit.code.clone());
    arrays.insert(
        "_meta".to_string(),
        serde_json::json!({
            "schema": SNAPSHOT_SCHEMA,
            "complete": complete,
            "captured": captured.clone(),
            "missing": missing.clone(),
            "limit": meta_limit,
            "game_pid": pid,
            "account": account,
            "cluster": cluster.map(|(start, end)| ClusterBounds { start, end }),
            "arrays": array_meta,
        }),
    );
    let mut error: Option<String> = None;
    match serde_json::to_string(&arrays) {
        Ok(json) => {
            // The raw snapshot is a local 0600 cache read by the app, not a
            // diagnostic snippet. Keep ItemType paths: redact_bare would
            // mistake long relic projection names for tokens.
            let json = redact_json(&json);
            if promote {
                // Write only when the content hash changed; the mtime is then
                // the time of the last content change, not of the last scan.
                // Offsets and the chosen cluster are diagnostics, so a moved
                // copy of the same inventory does not rewrite the raw.
                let previous_hash = previous_status
                    .as_ref()
                    .map(|status| status.content_hash.clone())
                    .filter(|hash| !hash.is_empty())
                    .or_else(|| raw_value.as_ref().and_then(content_hash_of_raw));
                // A missing or corrupt raw must be replaced even when the
                // status still claims the same content hash.
                if previous_unreadable
                    || previous_redacted
                    || trigger == "manual-rewrite"
                    || !raw_path.is_file()
                    || previous_hash.as_deref() != Some(candidate_hash.as_str())
                {
                    match write_inventory(&json) {
                        Ok(()) => {
                            println!("snapshot: wrote inventory-raw.json ({} bytes)", json.len())
                        }
                        Err(error_message) => {
                            eprintln!("snapshot: {error_message}");
                            error = Some(error_message);
                        }
                    }
                }
            } else {
                // A partial scan must never replace a good snapshot; it is only
                // diagnostics for the missing arrays.
                let partial = fetch::cache_dir().join("inventory-raw.partial.json");
                if let Err(write_error) = write_private(&partial, json.as_bytes()) {
                    let message = format!("{}: {write_error}", partial.display());
                    eprintln!("snapshot: {message}");
                    error.get_or_insert(message);
                }
            }
        }
        Err(serialize_error) => {
            let message = format!("inventory json: {serialize_error}");
            eprintln!("snapshot: {message}");
            error = Some(message);
        }
    }

    let promoted = promote && error.is_none();
    let duration_ms = started.elapsed().as_millis() as u64;
    let extraction_bytes = EXTRACTION_BUDGET - budget;
    let limit_blocking = limit.as_ref().is_some_and(|limit| limit.is_blocking());
    let captured_at = raw_mtime()
        .or_else(|| previous_status.as_ref().map(|status| status.captured_at))
        .unwrap_or(0);
    let verified_at = if promoted {
        scanned_at
    } else {
        previous_status
            .as_ref()
            .map(|status| status.verified_at)
            .unwrap_or(0)
    };
    let content_hash = if promoted {
        candidate_hash
    } else {
        previous_status
            .as_ref()
            .map(|status| status.content_hash.clone())
            .or_else(|| raw_value.as_ref().and_then(content_hash_of_raw))
            .unwrap_or_default()
    };
    let fast_cache = status_cache(
        promoted,
        cluster,
        &profile_regions,
        previous_status.as_ref(),
        pid,
    );
    let status = StatusFile {
        version: snapshot_status::STATUS_VERSION,
        snapshot_schema: SNAPSHOT_SCHEMA,
        content_hash,
        captured_at,
        verified_at,
        last_scan_at: scanned_at,
        // A successful promote replaces the corrupt raw, so the condition is
        // resolved for this scan.
        previous_unreadable: previous_unreadable && !promoted,
        game_pid: pid,
        fast_cluster: fast_cache.cluster,
        fast_regions: fast_cache.region,
        last_scan: LastScan {
            complete,
            limit: meta_limit,
            limit_blocking,
            trigger: trigger.to_string(),
            fast_path: fast_path.as_str().to_string(),
            bytes: state.0,
            extraction_bytes,
            regions: selected.len(),
            duration_ms,
            error: error.clone(),
        },
    };

    Ok(ScanOutcome {
        report: ScanReport {
            pid,
            game: cmdline(pid).chars().take(512).collect(),
            scanned_at,
            captured_at,
            duration_ms,
            regions: selected.len(),
            bytes: state.0,
            extraction_bytes,
            skipped_regions,
            skipped_bytes,
            truncated: state.2,
            complete,
            captured,
            missing,
            limit,
            trigger: trigger.to_string(),
            fast_path,
            cluster: cluster.map(|(start, end)| ClusterBounds { start, end }),
            profile_regions,
            hits_total: hits.len(),
            key_hits: hits_report,
            extraction: extraction_usage,
            markers: hits,
            error,
        },
        status,
        promoted,
    })
}

fn write_report(report: &ScanReport) -> std::result::Result<(), String> {
    let dir = fetch::cache_dir();
    ensure_private_dir(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let path = dir.join("inventory-scan.json");
    let mut value =
        serde_json::to_value(report).map_err(|error| format!("report json: {error}"))?;
    // A steady scan repeats the same offsets thousands of times. Counts stay
    // complete; the per-hit detail is what the file spent its megabytes on.
    if std::env::var_os("CORUFRAME_SNAPSHOT_DEBUG").is_none() {
        if let Some(markers) = value
            .get_mut("markers")
            .and_then(serde_json::Value::as_array_mut)
        {
            markers.truncate(MAX_REPORT_MARKERS);
        }
        if let Some(key_hits) = value
            .get_mut("key_hits")
            .and_then(serde_json::Value::as_array_mut)
        {
            for key in key_hits {
                if let Some(offsets) = key
                    .get_mut("offsets")
                    .and_then(serde_json::Value::as_array_mut)
                {
                    offsets.truncate(MAX_REPORT_OFFSETS);
                }
            }
        }
    }
    let json =
        serde_json::to_vec_pretty(&value).map_err(|error| format!("report json: {error}"))?;
    write_private(&path, &json).map_err(|error| format!("{}: {error}", path.display()))
}

fn trigger_name(line: &str) -> Option<&'static str> {
    for marker in LOG_TRIGGERS {
        if line.contains(marker) {
            return Some(match marker {
                "Logged in " | "MainMenu::LoginDone" | "OnLoginDone" | "CompleteLoginTasks" => {
                    "login"
                }
                _ => "load",
            });
        }
    }
    None
}

fn error_status(error: &str, trigger: &str, pid: u32) -> StatusFile {
    let previous = read_status();
    let same_pid = previous
        .as_ref()
        .is_some_and(|status| status.game_pid == pid);
    StatusFile {
        version: snapshot_status::STATUS_VERSION,
        snapshot_schema: SNAPSHOT_SCHEMA,
        content_hash: previous
            .as_ref()
            .map(|status| status.content_hash.clone())
            .unwrap_or_default(),
        captured_at: raw_mtime()
            .or_else(|| previous.as_ref().map(|status| status.captured_at))
            .unwrap_or(0),
        verified_at: previous
            .as_ref()
            .map(|status| status.verified_at)
            .unwrap_or(0),
        last_scan_at: now(),
        previous_unreadable: previous
            .as_ref()
            .is_some_and(|status| status.previous_unreadable),
        game_pid: pid,
        fast_cluster: if same_pid {
            previous.as_ref().and_then(|status| status.fast_cluster)
        } else {
            None
        },
        fast_regions: if same_pid {
            previous
                .as_ref()
                .map(|status| status.fast_regions.clone())
                .unwrap_or_default()
        } else {
            Vec::new()
        },
        last_scan: LastScan {
            complete: false,
            limit: Some("scan_error".to_string()),
            limit_blocking: true,
            trigger: trigger.to_string(),
            fast_path: "none".to_string(),
            bytes: 0,
            extraction_bytes: 0,
            regions: 0,
            duration_ms: 0,
            error: Some(error.to_string()),
        },
    }
}

// A login/load marker means the game is already syncing, so an event scan may
// skip the settle. The first read of the log replays the previous session and
// must not arm anything.
struct CadenceState {
    events: bool,
    scan_every: Duration,
    settle: Duration,
    burst_until: Option<Instant>,
    pending_trigger: Option<&'static str>,
    interval: Duration,
    catch_up: bool,
}

impl CadenceState {
    fn new(settle: Duration, scan_every: Duration, events: bool) -> Self {
        Self {
            events,
            scan_every,
            settle,
            burst_until: None,
            pending_trigger: None,
            interval: scan_every,
            catch_up: false,
        }
    }

    fn observe_line(&mut self, line: &str, historical: bool, now: Instant) {
        if historical || !self.events {
            return;
        }
        if let Some(trigger) = trigger_name(line) {
            self.burst_until = Some(now + BURST);
            self.pending_trigger = Some(trigger);
        }
    }

    fn on_truncation(&mut self, now: Instant) {
        self.pending_trigger = None;
        if self.events {
            self.burst_until = Some(now + BURST);
        }
    }

    fn on_pid_loss(&mut self) {
        self.pending_trigger = None;
        self.burst_until = None;
    }

    fn request_manual(&mut self, rewrite: bool) {
        self.pending_trigger = Some(if rewrite { "manual-rewrite" } else { "manual" });
        self.burst_until = Some(Instant::now() + BURST);
    }

    fn record_scan(&mut self, promoted: bool) {
        if promoted {
            // The sync was captured; more reads of the same burst add nothing.
            self.burst_until = None;
            self.interval = if self.events {
                FULL_FALLBACK
            } else {
                self.scan_every
            };
        } else {
            self.interval = INCOMPLETE_RETRY;
        }
    }

    fn due(&self, now: Instant, found_at: Instant, last_scan: Instant) -> bool {
        let settled = now.duration_since(found_at) >= self.settle || self.pending_trigger.is_some();
        if !settled {
            return false;
        }
        let bursting = self.burst_until.is_some_and(|until| now < until);
        let effective = if bursting {
            BURST_INTERVAL.min(self.interval)
        } else {
            self.interval
        };
        now.duration_since(last_scan) >= effective
    }

    fn take_trigger(&mut self) -> &'static str {
        self.pending_trigger.take().unwrap_or("timer")
    }

    fn take_catch_up(&mut self) -> bool {
        std::mem::replace(&mut self.catch_up, false)
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct FastCache {
    cluster: Option<(u64, u64)>,
    region: Vec<(u64, u64)>,
}

impl FastCache {
    fn is_empty(&self) -> bool {
        self.cluster.is_none() && self.region.is_empty()
    }
}

fn cache_from_status(pid: u32, status: &StatusFile) -> FastCache {
    // A cache recorded under an older schema may not cover arrays that are
    // required now, so reusing it would make the first timer scans incomplete.
    // Discarding it forces one full scan, which records the new regions.
    if status.game_pid != pid || status.snapshot_schema != SNAPSHOT_SCHEMA {
        return FastCache::default();
    }
    FastCache {
        cluster: status.fast_cluster,
        region: status.fast_regions.clone(),
    }
}

// A wrapper restart should not pay for a full scan when the same game process
// is still alive. Bounds are revalidated against the live maps on the first
// use, so a stale entry only causes a miss, never a wrong snapshot.
fn seed_cache(pid: u32) -> FastCache {
    read_status().map_or_else(FastCache::default, |status| cache_from_status(pid, &status))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PidEvent {
    None,
    Appeared(u32),
    Replaced(u32),
    Lost(u32),
}

fn observe_pid(game: &mut Option<(u32, Instant)>, found: Option<u32>, now: Instant) -> PidEvent {
    let previous = game.map(|(pid, _)| pid);
    match (previous, found) {
        (Some(previous), Some(found)) if previous == found => PidEvent::None,
        (Some(_), Some(found)) => {
            *game = Some((found, now));
            PidEvent::Replaced(found)
        }
        (Some(previous), None) => {
            *game = None;
            PidEvent::Lost(previous)
        }
        (None, Some(found)) => {
            *game = Some((found, now));
            PidEvent::Appeared(found)
        }
        (None, None) => PidEvent::None,
    }
}

fn reset_session(event: PidEvent, cache: &mut FastCache, misses: &mut u32) -> bool {
    if event == PidEvent::None {
        return false;
    }
    *cache = FastCache::default();
    *misses = 0;
    true
}

struct WriteGate {
    until: Option<Instant>,
}

impl WriteGate {
    fn new() -> Self {
        Self { until: None }
    }

    fn ready(&self, now: Instant) -> bool {
        self.until.is_none_or(|until| now >= until)
    }

    fn failed(&mut self, now: Instant) {
        self.until = Some(now + WRITE_BACKOFF);
    }

    fn ok(&mut self) {
        self.until = None;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanTier {
    Full,
    Cluster,
    Region,
}

fn next_tier(trigger: &str, fast: bool, cache: &FastCache, misses: u32) -> ScanTier {
    if !fast || trigger != "timer" || cache.is_empty() || misses >= FAST_MISSES_BEFORE_FULL {
        return ScanTier::Full;
    }
    if cache.cluster.is_some() {
        ScanTier::Cluster
    } else {
        ScanTier::Region
    }
}

struct CycleResult {
    outcome: ScanOutcome,
    // A fast-path scan that did not promote; counted until a full scan is due.
    miss: bool,
    full: bool,
}

fn scan_cycle(
    pid: u32,
    account: Option<&str>,
    cache: &FastCache,
    trigger: &str,
    fast: bool,
    misses: u32,
) -> std::result::Result<CycleResult, String> {
    let full = || scan(pid, account, None, trigger, FastPath::None);
    match next_tier(trigger, fast, cache, misses) {
        ScanTier::Full => Ok(CycleResult {
            outcome: full()?,
            miss: false,
            full: true,
        }),
        ScanTier::Cluster => {
            let cluster = cache.cluster.expect("cluster tier needs a cluster");
            let first = scan(pid, account, Some(&[cluster]), trigger, FastPath::Cluster)?;
            if first.promoted {
                return Ok(CycleResult {
                    outcome: first,
                    miss: false,
                    full: false,
                });
            }
            if cache.region.is_empty() {
                // Nothing wider is cached, so a timer miss stays a miss rather
                // than paying for a full read every tick.
                return Ok(CycleResult {
                    outcome: first,
                    miss: true,
                    full: false,
                });
            }
            let second = scan(pid, account, Some(&cache.region), trigger, FastPath::Region)?;
            if second.promoted {
                return Ok(CycleResult {
                    outcome: second,
                    miss: false,
                    full: false,
                });
            }
            if region_gone(&first.report) || region_gone(&second.report) {
                return Ok(CycleResult {
                    outcome: full()?,
                    miss: false,
                    full: true,
                });
            }
            Ok(CycleResult {
                outcome: second,
                miss: true,
                full: false,
            })
        }
        ScanTier::Region => {
            let first = scan(pid, account, Some(&cache.region), trigger, FastPath::Region)?;
            if first.promoted {
                return Ok(CycleResult {
                    outcome: first,
                    miss: false,
                    full: false,
                });
            }
            if region_gone(&first.report) {
                return Ok(CycleResult {
                    outcome: full()?,
                    miss: false,
                    full: true,
                });
            }
            Ok(CycleResult {
                outcome: first,
                miss: true,
                full: false,
            })
        }
    }
}

fn region_gone(report: &ScanReport) -> bool {
    report
        .limit
        .as_ref()
        .is_some_and(|limit| limit.code == "cached_region_gone")
}

#[derive(Clone, PartialEq, Eq)]
struct ReportState {
    complete: bool,
    limit: Option<String>,
    trigger: String,
    fast_path: FastPath,
}

impl ReportState {
    fn of(report: &ScanReport) -> Self {
        Self {
            complete: report.complete,
            limit: report.limit.as_ref().map(|limit| limit.code.clone()),
            trigger: report.trigger.clone(),
            fast_path: report.fast_path,
        }
    }
}

fn should_write_report(
    report: &ScanReport,
    last_at: Option<Instant>,
    last_state: Option<&ReportState>,
) -> bool {
    // A full scan is the expensive one and the one worth keeping in full.
    if report.fast_path == FastPath::None {
        return true;
    }
    if last_state != Some(&ReportState::of(report)) {
        return true;
    }
    last_at.is_none_or(|at| at.elapsed() >= REPORT_INTERVAL)
}

fn scanner_loop(stop: Arc<AtomicBool>, settle: u64, scan_every: u64) {
    let wrapper = std::process::id();
    let events = std::env::var("CORUFRAME_SCAN_EVENTS").map_or(true, |value| value != "0");
    let fast = std::env::var("CORUFRAME_SCAN_FAST").map_or(true, |value| value != "0");
    let mut game: Option<(u32, Instant)> = None;
    let mut follower: Option<Follower> = None;
    let mut follower_error: Option<String> = None;
    let mut account: Option<String> = None;
    let mut cache = FastCache::default();
    let mut fast_misses = 0u32;
    let mut last_scan = Instant::now() - Duration::from_secs(scan_every);
    let mut last_report_at: Option<Instant> = None;
    let mut last_report_state: Option<ReportState> = None;
    let mut status_gate = WriteGate::new();
    let mut cadence = CadenceState::new(
        Duration::from_secs(settle),
        Duration::from_secs(scan_every),
        events,
    );

    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let request = coruframe_core::snapshot_status::scan_request_path();
        if request.is_file() {
            let rewrite = std::fs::read_to_string(&request)
                .map(|body| body.trim() == "rewrite")
                .unwrap_or(false);
            let _ = std::fs::remove_file(&request);
            cadence.request_manual(rewrite);
        }
        if follower.is_none() {
            if let Some(path) = logpath::find() {
                println!("snapshot: following {}", path.display());
                follower = Some(Follower::new(path, true));
                cadence.catch_up = true;
                follower_error = None;
            }
        }
        let mut reopen = false;
        let follower_path = follower
            .as_ref()
            .map(|current| current.path().to_path_buf());
        if let Some(current) = follower.as_mut() {
            match current.poll() {
                Ok(lines) => {
                    follower_error = None;
                    let historical = cadence.take_catch_up();
                    if current.last_poll_truncated() {
                        // A new launch: the old login's carry key is stale.
                        account = None;
                        cache = FastCache::default();
                        fast_misses = 0;
                        cadence.on_truncation(Instant::now());
                    }
                    for line in lines {
                        if let Some(alias) = account::alias_from_line(&line) {
                            account = Some(account::id(&alias));
                        }
                        cadence.observe_line(&line, historical, Instant::now());
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    // A missing log is polled every two seconds; log the change
                    // of state once, not each failure.
                    if follower_error.as_deref() != Some(message.as_str()) {
                        if let Some(path) = &follower_path {
                            eprintln!("snapshot: follow {}: {message}", path.display());
                        }
                        follower_error = Some(message);
                    }
                    reopen = true;
                }
            }
        }
        if reopen {
            follower = None;
        }

        let alive =
            game.is_some_and(|(pid, _)| std::path::Path::new(&format!("/proc/{pid}")).exists());
        let found = if alive {
            game.map(|(pid, _)| pid)
        } else {
            find_game_pid(wrapper)
        };
        let event = observe_pid(&mut game, found, Instant::now());
        if reset_session(event, &mut cache, &mut fast_misses) {
            match event {
                PidEvent::Appeared(pid) => {
                    println!("snapshot: found Warframe pid {pid}");
                    cache = seed_cache(pid);
                    last_scan = Instant::now() - Duration::from_secs(scan_every);
                }
                PidEvent::Replaced(pid) => {
                    println!("snapshot: switched Warframe pid to {pid}");
                    cadence.on_pid_loss();
                    cache = seed_cache(pid);
                    last_scan = Instant::now() - Duration::from_secs(scan_every);
                }
                PidEvent::Lost(pid) => {
                    println!("snapshot: lost Warframe pid {pid}");
                    cadence.on_pid_loss();
                }
                PidEvent::None => {}
            }
        }
        if let Some((pid, found)) = game {
            if cadence.due(Instant::now(), found, last_scan) {
                let trigger = cadence.take_trigger();
                match scan_cycle(pid, account.as_deref(), &cache, trigger, fast, fast_misses) {
                    Ok(result) => {
                        let report = &result.outcome.report;
                        println!(
                            "snapshot: complete={} missing=[{}] hits={} (pid {pid}, {} regions, {} MB, trigger={} fast={}, limit={:?}){}",
                            report.complete,
                            report.missing.join(", "),
                            report.hits_total,
                            report.regions,
                            report.bytes / (1024 * 1024),
                            report.trigger,
                            report.fast_path.as_str(),
                            report.limit.as_ref().map(|limit| limit.code.as_str()),
                            if report.truncated { " (truncated)" } else { "" }
                        );
                        cadence.record_scan(result.outcome.promoted);
                        if result.full {
                            fast_misses = 0;
                        } else if result.miss {
                            fast_misses += 1;
                        } else if result.outcome.promoted {
                            fast_misses = 0;
                        }
                        if result.outcome.promoted && report.fast_path != FastPath::Cluster {
                            if let Some(cluster) = report.cluster {
                                cache.cluster = Some((cluster.start, cluster.end));
                            }
                            if !report.profile_regions.is_empty() {
                                cache.region = report.profile_regions.clone();
                            }
                        }
                        if status_gate.ready(Instant::now()) {
                            match write_status(&result.outcome.status) {
                                Ok(()) => status_gate.ok(),
                                Err(error) => {
                                    eprintln!("snapshot: status: {error}");
                                    status_gate.failed(Instant::now());
                                }
                            }
                        }
                        let state = ReportState::of(report);
                        if should_write_report(report, last_report_at, last_report_state.as_ref()) {
                            if let Err(error) = write_report(report) {
                                eprintln!("snapshot: report: {error}");
                            }
                            last_report_at = Some(Instant::now());
                            last_report_state = Some(state);
                        }
                    }
                    Err(error) => {
                        eprintln!("snapshot: {error}");
                        cadence.record_scan(false);
                        if status_gate.ready(Instant::now()) {
                            if let Err(write_error) =
                                write_status(&error_status(&error, trigger, pid))
                            {
                                eprintln!("snapshot: status: {write_error}");
                                status_gate.failed(Instant::now());
                            } else {
                                status_gate.ok();
                            }
                        }
                    }
                }
                last_scan = Instant::now();
            }
        }
        thread::sleep(Duration::from_secs(2));
    }
}

pub fn launch(args: &[String]) -> Result<()> {
    let mut scan_every = 20u64;
    let mut settle = 25u64;
    let mut command = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--scan-every" => {
                index += 1;
                scan_every = args
                    .get(index)
                    .ok_or("--scan-every needs seconds")?
                    .parse()
                    .map_err(|error| format!("--scan-every: {error}"))?;
            }
            "--settle" => {
                index += 1;
                settle = args
                    .get(index)
                    .ok_or("--settle needs seconds")?
                    .parse()
                    .map_err(|error| format!("--settle: {error}"))?;
            }
            "--" => {
                command.extend_from_slice(&args[index + 1..]);
                break;
            }
            _ => {
                command.extend_from_slice(&args[index..]);
                break;
            }
        }
        index += 1;
    }
    if command.is_empty() {
        return Err("launch needs a command; use `coruframe launch -- <command>`".into());
    }

    let mut child = Command::new(&command[0])
        .args(&command[1..])
        .spawn()
        .map_err(|error| format!("launch {}: {error}", command[0]))?;
    println!(
        "snapshot: wrapper pid {}, child pid {}",
        std::process::id(),
        child.id()
    );

    let stop = Arc::new(AtomicBool::new(false));
    let blocked = coruframe_core::capabilities::snapshot_block_reason();
    if let Some(reason) = &blocked {
        eprintln!("snapshot: disabled: {reason}");
    }
    let scanner = if blocked.is_some() {
        None
    } else {
        let scanner_stop = stop.clone();
        Some(thread::spawn(move || {
            scanner_loop(scanner_stop, settle, scan_every)
        }))
    };
    // The wrapper is only the game's parent (snapshot/read-only). The reward
    // probe is a desktop-session process: bundling it here made it inherit a
    // gamescope environment and the wrong portal identity.
    let status = child.wait()?;
    stop.store(true, Ordering::SeqCst);
    if let Some(scanner) = scanner {
        let _ = scanner.join();
    }
    std::process::exit(status.code().unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_anonymous_writable_regions() {
        let (start, end, writable, path) =
            parse_map("55d0-55d1 rw-p 00000000 00:00 0 [heap]").unwrap();
        assert_eq!(
            (start, end, writable, path.as_str()),
            (0x55d0, 0x55d1, true, "[heap]")
        );
    }

    #[test]
    fn rejects_read_only_regions() {
        let (_, _, writable, _) = parse_map("55d0-55d1 r--p 00000000 00:00 0").unwrap();
        assert!(!writable);
    }

    #[test]
    fn decodes_utf16le_snippets() {
        let bytes = [b'"', 0, b'I', 0, b't', 0];
        assert_eq!(decode_snippet(&bytes, true), "\"It");
    }

    #[test]
    fn searches_both_encodings() {
        let definitions = markers();
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "ItemCount" && !marker.utf16));
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "ItemCount" && marker.utf16));
        assert!(definitions.iter().any(|marker| marker.name == "XPInfo"));
        // Sentinel weapons are a separate profile array, not part of LongGuns.
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "SentinelWeapons" && !marker.utf16));
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "Sentinels" && !marker.utf16));
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "KubrowPets" && !marker.utf16));
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "MechSuits" && !marker.utf16));
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "SpaceGuns" && !marker.utf16));
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "SpaceMelee" && !marker.utf16));
        assert!(definitions
            .iter()
            .any(|marker| marker.name == "OperatorAmps" && !marker.utf16));
    }
}

#[cfg(test)]
mod redaction_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn redacts_tokens_and_ids_but_keeps_inventory_keys() {
        let text = r#"{"Token":"secret","SteamId":"123","id":"abc","ItemType":"/Lotus/Test","ItemCount":1}"#;
        let redacted = redact(text);
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("\"123\""));
        assert!(!redacted.contains("\"abc\""));
        assert!(redacted.contains(r#""ItemType":"/Lotus/Test""#));
        assert!(redacted.contains(r#""ItemCount":1"#));
        assert!(redacted.contains("[redacted]"));
    }

    #[test]
    fn detects_a_raw_written_by_the_old_redactor() {
        assert!(contains_redacted(
            br#"{"ItemType":"/Lotus/Types/Game/Projections/[redacted]"}"#
        ));
        assert!(!contains_redacted(
            br#"{"ItemType":"/Lotus/Types/Game/Projections/AxiA1"}"#
        ));
    }

    #[test]
    fn raw_redaction_keeps_long_relic_paths() {
        let text = r#"{"ItemType":"/Lotus/Types/Game/Projections/T4VoidProjectionMagNovaVaultB","ItemCount":2,"Token":"secret"}"#;
        let redacted = redact_json(text);
        assert!(
            redacted.contains("/Lotus/Types/Game/Projections/T4VoidProjectionMagNovaVaultB"),
            "{redacted}"
        );
        assert!(!redacted.contains("secret"), "{redacted}");
    }

    #[test]
    fn redacts_sensitive_keys_nested_in_objects_and_arrays() {
        let text =
            r#"{"data":{"token":"secret","name":"ok","list":[{"session":"abc"}],"ItemCount":1}}"#;
        let redacted = redact(text);
        assert!(!redacted.contains("secret"), "{redacted}");
        assert!(!redacted.contains("abc"), "{redacted}");
        assert!(redacted.contains(r#""name":"ok""#), "{redacted}");
        assert!(redacted.contains(r#""ItemCount":1"#), "{redacted}");
    }

    #[test]
    fn redacts_numeric_and_container_values() {
        let text =
            r#"{"accountId":12345,"Session":[1,2],"secret":{"a":1},"visible":true,"ItemCount":7}"#;
        let redacted = redact(text);
        assert!(!redacted.contains("12345"));
        assert!(!redacted.contains("[1,2]"));
        assert!(!redacted.contains(r#"{"a":1}"#));
        assert!(redacted.contains(r#""visible":true"#));
        assert!(redacted.contains(r#""ItemCount":7"#));
        assert_eq!(redacted.matches("[redacted]").count(), 3);
    }

    #[test]
    fn redacts_bare_emails_and_tokens_outside_json() {
        let text = "mail tenno@example.com token eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123def456ghi";
        let redacted = redact(text);
        assert!(!redacted.contains("tenno@example.com"), "{redacted}");
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9"), "{redacted}");
        assert!(redacted.contains("mail"));
        assert!(redacted.contains("token"));
    }

    #[test]
    fn redacts_a_utf16_email_snippet() {
        let text = "user@example.com";
        let utf16: Vec<u8> = text.bytes().flat_map(|byte| [byte, 0]).collect();
        let decoded = decode_snippet(&utf16, true);
        assert!(!decoded.contains("user@example.com"), "{decoded}");
        assert!(decoded.contains("[redacted]"));
    }

    #[test]
    fn keeps_short_names_and_paths() {
        let text = "/Lotus/StoreItems/Weapons/FulminPrimeBarrel Vadarya Prime Receiver";
        assert_eq!(redact(text), text);
    }

    #[test]
    fn writes_snapshot_files_private() {
        let dir = std::env::temp_dir().join(format!("coruframe-mode-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("inventory-raw.json");
        let _ = fs::remove_file(&path);
        write_private(&path, b"{}").unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod extraction_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn recognizes_item_arrays_and_skips_object_id_lists() {
        assert!(is_item_array(&serde_json::json!([
            {"ItemType": "/Lotus/Test", "ItemCount": 1}
        ])));
        assert!(!is_item_array(&serde_json::json!([
            "649cc699823660552001cb9e"
        ])));
    }

    fn temp_file(name: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("coruframe-{}-{name}", std::process::id()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn detects_object_arrays_but_rejects_string_arrays() {
        let object = r#"{"Upgrades":[{"ItemType":"/Lotus/Test"}]}"#;
        let path = temp_file("object.json", object);
        let file = std::fs::File::open(&path).unwrap();
        let offset = object.find("\"Upgrades\"").unwrap() as u64;
        assert!(object_array_start(&file, offset, object.len() as u64, true));
        let _ = std::fs::remove_file(&path);

        let strings = r#"{"Upgrades":["649cc699823660552001cb9e"]}"#;
        let path = temp_file("strings.json", strings);
        let file = std::fs::File::open(&path).unwrap();
        let offset = strings.find("\"Upgrades\"").unwrap() as u64;
        assert!(!object_array_start(
            &file,
            offset,
            strings.len() as u64,
            true
        ));
        let _ = std::fs::remove_file(&path);

        let empty = r#"{"MechSuits":[]}"#;
        let path = temp_file("empty.json", empty);
        let file = std::fs::File::open(&path).unwrap();
        let offset = empty.find("\"MechSuits\"").unwrap() as u64;
        assert!(object_array_start(&file, offset, empty.len() as u64, true));
        assert!(!object_array_start(
            &file,
            offset,
            empty.len() as u64,
            false
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extraction_stops_at_the_budget() {
        let text = r#"{"Upgrades":[{"ItemType":"/Lotus/Test","ItemCount":1}]}"#;
        let path = temp_file("budget.json", text);
        let file = std::fs::File::open(&path).unwrap();
        let offset = text.find("\"Upgrades\"").unwrap() as u64;
        let end = text.len() as u64;

        let mut empty = 0;
        assert_eq!(
            extract_array(&file, 0, end, offset, "Upgrades", &mut empty),
            Err(ExtractMiss::Budget)
        );

        let mut budget = 1 << 20;
        assert!(extract_array(&file, 0, end, offset, "Upgrades", &mut budget).is_ok());
        assert!(budget < 1 << 20);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extraction_window_is_bounded_per_hit() {
        let path = std::env::temp_dir().join(format!(
            "coruframe-window-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let text = r#"{"Suits":[{"ItemType":"/L/Suit"}]}"#;
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, text.as_bytes()).unwrap();
        let region_end = 16 << 20;
        file.set_len(region_end).unwrap();
        let offset = text.find("\"Suits\"").unwrap() as u64;
        let mut budget = EXTRACTION_BUDGET;
        let value = extract_array(&file, 0, region_end, offset, "Suits", &mut budget).unwrap();
        assert!(is_item_array(&value));
        assert_eq!(EXTRACTION_BUDGET - budget, offset + EXTRACTION_WINDOW);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extraction_takes_the_first_complete_copy() {
        let text = concat!(
            r#"{"Suits":[{"ItemType":"/L/First"}]}"#,
            r#"{"Suits":[{"ItemType":"/L/Second"},{"ItemType":"/L/Second2"}]}"#,
        );
        let path = temp_file("copies.json", text);
        let file = std::fs::File::open(&path).unwrap();
        let first = text.find("\"Suits\"").unwrap() as u64;
        let second = text.rfind("\"Suits\"").unwrap() as u64;
        let hits = [
            MarkerHit {
                name: "Suits".to_string(),
                encoding: "utf-8",
                offset: first,
                snippet: String::new(),
            },
            MarkerHit {
                name: "Suits".to_string(),
                encoding: "utf-8",
                offset: second,
                snippet: String::new(),
            },
        ];
        let mut budget = EXTRACTION_BUDGET;
        let (value, offset) = extract_key(
            &file,
            &[(0, text.len() as u64)],
            &hits,
            "Suits",
            None,
            &mut budget,
        )
        .unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(offset, first);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn empty_inventory_arrays_are_captured_and_non_empty_wins() {
        let text = concat!(
            r#"{"MechSuits":[]}"#,
            r#"{"MechSuits":[{"ItemType":"/L/Voidrig","ItemCount":1}]}"#,
        );
        let path = temp_file("empty-copies.json", text);
        let file = std::fs::File::open(&path).unwrap();
        let first = text.find("\"MechSuits\"").unwrap() as u64;
        let second = text.rfind("\"MechSuits\"").unwrap() as u64;
        let hits = [
            MarkerHit {
                name: "MechSuits".to_string(),
                encoding: "utf-8",
                offset: first,
                snippet: String::new(),
            },
            MarkerHit {
                name: "MechSuits".to_string(),
                encoding: "utf-8",
                offset: second,
                snippet: String::new(),
            },
        ];
        let mut budget = EXTRACTION_BUDGET;
        let (value, offset) = extract_key(
            &file,
            &[(0, text.len() as u64)],
            &hits,
            "MechSuits",
            None,
            &mut budget,
        )
        .unwrap();
        assert_eq!(value.as_array().unwrap().len(), 1);
        assert_eq!(offset, second);
        let _ = std::fs::remove_file(&path);

        let only_empty = r#"{"MechSuits":[]}"#;
        let path = temp_file("only-empty.json", only_empty);
        let file = std::fs::File::open(&path).unwrap();
        let offset = only_empty.find("\"MechSuits\"").unwrap() as u64;
        let hits = [MarkerHit {
            name: "MechSuits".to_string(),
            encoding: "utf-8",
            offset,
            snippet: String::new(),
        }];
        let mut budget = EXTRACTION_BUDGET;
        let (value, _) = extract_key(
            &file,
            &[(0, only_empty.len() as u64)],
            &hits,
            "MechSuits",
            None,
            &mut budget,
        )
        .unwrap();
        assert!(value.as_array().unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cluster_prefers_the_copy_with_more_core_keys() {
        let hit = |name: &str, offset: u64| MarkerHit {
            name: name.to_string(),
            encoding: "utf-8",
            offset,
            snippet: String::new(),
        };
        let hits = [
            hit("MiscItems", 100),
            hit("LongGuns", 200),
            hit("Suits", 300),
            hit("MiscItems", 10_000_000),
        ];
        let (start, end) = cluster_span(&hits).unwrap();
        assert!(start <= 100 && 300 < end);
        assert!(!(start <= 10_000_000 && 10_000_000 < end));
    }

    #[test]
    fn fills_absent_optional_arrays_from_the_last_snapshot_or_empty() {
        let mut arrays = serde_json::json!({
            "MiscItems": [{"ItemType": "/L/Misc"}],
            "Suits": [{"ItemType": "/L/Suit"}],
            "LongGuns": [{"ItemType": "/L/Gun"}],
            "Pistols": [{"ItemType": "/L/Pistol"}],
            "Melee": [{"ItemType": "/L/Melee"}],
            "XPInfo": [{"ItemType": "/L/Suit", "XP": 1}],
            "RawUpgrades": [],
            "Upgrades": [],
        })
        .as_object()
        .unwrap()
        .clone();
        let previous = serde_json::json!({
            "KubrowPets": [{"ItemType": "/L/Pet", "ItemCount": 1}],
            "SpaceSuits": [],
            "_meta": {"complete": true}
        })
        .as_object()
        .unwrap()
        .clone();

        let filled = fill_missing_optional(&mut arrays, &previous, &[], true, Some(1_000));
        assert_eq!(arrays["KubrowPets"][0]["ItemType"], "/L/Pet");
        assert!(arrays["SpaceSuits"].as_array().unwrap().is_empty());
        assert!(arrays["MechSuits"].as_array().unwrap().is_empty());
        assert!(filled
            .iter()
            .any(|(key, source, _)| key == "KubrowPets" && *source == "carried"));
        assert!(filled
            .iter()
            .any(|(key, source, _)| key == "MechSuits" && *source == "synthesized"));
        assert_eq!(
            filled
                .iter()
                .find(|(key, _, _)| key == "KubrowPets")
                .and_then(|(_, _, observed)| *observed),
            Some(1_000)
        );
        // Synthesized absence is not observed data; the snapshot clocks carry
        // the time instead, keeping the content hash stable across scans.
        assert_eq!(
            filled
                .iter()
                .find(|(key, _, _)| key == "MechSuits")
                .and_then(|(_, _, observed)| *observed),
            None
        );
        let (complete, _, missing) = snapshot_status(&arrays);
        assert!(complete);
        assert!(missing.is_empty());
    }

    #[test]
    fn a_present_but_unreadable_optional_array_is_not_invented_as_empty() {
        let mut arrays = serde_json::json!({
            "MiscItems": [], "Suits": [], "LongGuns": [], "Pistols": [],
            "Melee": [], "XPInfo": []
        })
        .as_object()
        .unwrap()
        .clone();
        let hits = [MarkerHit {
            name: "KubrowPets".to_string(),
            encoding: "utf-8",
            offset: 1,
            snippet: String::new(),
        }];
        fill_missing_optional(&mut arrays, &serde_json::Map::new(), &hits, true, None);
        assert!(!arrays.contains_key("KubrowPets"));
        assert!(arrays.contains_key("SpaceSuits"));
    }

    #[test]
    fn blocked_synthesis_leaves_the_key_missing_but_still_carries() {
        let mut arrays = serde_json::json!({
            "MiscItems": [], "Suits": [], "LongGuns": [], "Pistols": [],
            "Melee": [], "XPInfo": []
        })
        .as_object()
        .unwrap()
        .clone();
        let previous = serde_json::json!({
            "KubrowPets": [{"ItemType": "/L/Pet"}]
        })
        .as_object()
        .unwrap()
        .clone();
        let filled = fill_missing_optional(&mut arrays, &previous, &[], false, Some(10));
        assert!(arrays.contains_key("KubrowPets"));
        assert!(!arrays.contains_key("SpaceSuits"));
        assert!(!arrays.contains_key("MechSuits"));
        assert!(filled.iter().all(|(_, source, _)| *source == "carried"));
        let (complete, _, missing) = snapshot_status(&arrays);
        assert!(!complete);
        assert!(missing.contains(&"SpaceSuits".to_string()));
    }

    #[test]
    fn a_corrupt_raw_is_not_carried_and_is_reported() {
        let (value, unreadable) = previous_raw(Some(b"{not json"));
        assert!(value.is_none());
        assert!(unreadable);
        let (value, unreadable) = previous_raw(Some(br#"{"Suits":[]}"#));
        assert!(value.is_some());
        assert!(!unreadable);
        let (value, unreadable) = previous_raw(Some(b"[]"));
        assert!(value.is_some());
        assert!(unreadable);
        let (value, unreadable) = previous_raw(None);
        assert!(value.is_none());
        assert!(!unreadable);
    }

    #[test]
    fn previous_arrays_requires_the_same_account_and_pid() {
        let raw = serde_json::json!({
            "_meta": { "game_pid": 7, "account": "aaaaaaaaaaaaaaaa" },
            "KubrowPets": []
        })
        .as_object()
        .unwrap()
        .clone();
        assert!(previous_arrays(Some(&raw), Some("aaaaaaaaaaaaaaaa"), 7).contains_key("KubrowPets"));
        assert!(previous_arrays(Some(&raw), Some("bbbbbbbbbbbbbbbb"), 7).is_empty());
        assert!(previous_arrays(Some(&raw), Some("aaaaaaaaaaaaaaaa"), 8).is_empty());
        assert!(previous_arrays(Some(&raw), None, 7).is_empty());
        assert!(previous_arrays(None, Some("aaaaaaaaaaaaaaaa"), 7).is_empty());
    }

    #[test]
    fn cluster_score_counts_only_distinct_tracked_keys_inside_the_span() {
        let hit = |name: &str, offset: u64| MarkerHit {
            name: name.to_string(),
            encoding: "utf-8",
            offset,
            snippet: String::new(),
        };
        let hits = [
            hit("MiscItems", 100),
            hit("MiscItems", 120),
            hit("Suits", 200),
            hit("Upgrades", 220),
            hit("LongGuns", 10_000),
        ];
        // Upgrades is tracked, so it counts like the inventory keys.
        assert_eq!(cluster_score(&hits, Some((0, 1_000))), 3);
        assert_eq!(cluster_score(&hits, None), 0);
    }

    #[test]
    fn selected_bounds_clips_a_cluster_to_its_live_mapping() {
        let available = [(0x1000, 0x2000), (0x5000, 0x6000)];
        let cached = [(0x1800, 0x1c00)];
        assert_eq!(
            selected_bounds(&available, Some(&cached)),
            [(0x1800, 0x1c00)]
        );
        // A cached region spanning two live mappings is clipped to both.
        assert_eq!(
            selected_bounds(&available, Some(&[(0x1800, 0x5800)])),
            [(0x1800, 0x2000), (0x5000, 0x5800)]
        );
        assert!(selected_bounds(&available, Some(&[(0x9000, 0xa000)])).is_empty());
        assert_eq!(selected_bounds(&available, None), available);
    }

    #[test]
    fn scans_shared_and_anonymous_regions_but_not_assets() {
        assert!(is_candidate_region(""));
        assert!(is_candidate_region("[heap]"));
        assert!(is_candidate_region("/memfd:warframe (deleted)"));
        assert!(is_candidate_region("/dev/shm/coruframe"));
        assert!(!is_candidate_region("/usr/lib/libfoo.so"));
        assert!(!is_candidate_region("[vdso]"));
        assert!(!is_candidate_region("[stack]"));
    }

    #[test]
    fn login_and_load_lines_start_a_burst() {
        for line in [
            "12.466 Sys [Info]: Logged in SyntheticTenno",
            "ThemedMainMenu.lua: MainMenu::LoginDone",
            "Background.lua: Background::OnLoginDone()",
            "LotusProfileData::CompleteLoginTasks",
        ] {
            assert_eq!(trigger_name(line), Some("login"), "{line}");
        }
        for line in [
            "ThemedSquadOverlay.lua: Lobby::Host_StartMatch",
            "Starting new background region:",
            "SetSquadHostHubLevelName:",
        ] {
            assert_eq!(trigger_name(line), Some("load"), "{line}");
        }
        assert_eq!(trigger_name("some unrelated line"), None);
    }

    #[test]
    fn report_serializes_limit_trigger_and_coverage() {
        let report = ScanReport {
            pid: 1,
            game: String::new(),
            scanned_at: 0,
            captured_at: 0,
            duration_ms: 0,
            regions: 0,
            bytes: 0,
            extraction_bytes: 0,
            skipped_regions: 2,
            skipped_bytes: 3,
            truncated: false,
            complete: false,
            captured: Vec::new(),
            missing: Vec::new(),
            limit: Some(Limit::blocking("scan_budget")),
            trigger: "login".to_string(),
            fast_path: FastPath::Cluster,
            cluster: Some(ClusterBounds { start: 4, end: 8 }),
            profile_regions: vec![(1, 2)],
            hits_total: 0,
            markers: Vec::new(),
            key_hits: Vec::new(),
            extraction: Vec::new(),
            error: None,
        };
        let value = serde_json::to_value(&report).unwrap();
        assert_eq!(value["limit"], "scan_budget");
        assert_eq!(value["trigger"], "login");
        assert_eq!(value["fast_path"], "cluster");
        assert_eq!(value["cluster"]["start"], 4);
        assert_eq!(value["skipped_regions"], 2);
        assert_eq!(value["skipped_bytes"], 3);
        assert_eq!(value["profile_regions"][0][0], 1);
    }

    #[test]
    fn every_tracked_key_has_a_marker() {
        let markers = markers();
        for key in inventory_keys::TRACKED {
            assert!(
                markers
                    .iter()
                    .any(|marker| marker.name == key && !marker.utf16),
                "no utf-8 marker for {key}"
            );
            assert!(
                markers
                    .iter()
                    .any(|marker| marker.name == key && marker.utf16),
                "no utf-16 marker for {key}"
            );
        }
    }

    #[test]
    fn recipes_is_not_extracted_anymore() {
        assert!(!extract_keys().any(|key| key == "Recipes"));
        assert!(!key_hits(&[]).iter().any(|hits| hits.name == "Recipes"));
        assert!(!inventory_keys::TRACKED.contains(&"Recipes"));
    }

    #[test]
    fn only_a_late_budget_miss_promotes_an_otherwise_complete_capture() {
        assert!(promotable(true, None, true));
        let late = Limit::new("scan_budget", false);
        assert!(promotable(true, Some(&late), true));
        assert!(!promotable(true, Some(&late), false));
        assert!(!promotable(false, Some(&late), true));
        let blocking = Limit::blocking("scan_budget");
        assert!(!promotable(true, Some(&blocking), true));
        let diagnostic = Limit::blocking("hit_cap:Suits");
        assert!(!promotable(false, Some(&diagnostic), true));
    }

    #[test]
    fn content_hash_ignores_offsets_but_tracks_provenance() {
        let arrays = serde_json::json!({
            "Suits": [{"ItemType": "/L/Suit", "ItemCount": 1}],
        })
        .as_object()
        .unwrap()
        .clone();
        let captured = serde_json::json!({
            "Suits": {"offset": 10, "in_cluster": true, "count": 1, "source": "captured"},
        })
        .as_object()
        .unwrap()
        .clone();
        let mut moved = captured.clone();
        moved["Suits"]["offset"] = serde_json::json!(99);
        assert_eq!(
            content_hash(&arrays, &captured, SNAPSHOT_SCHEMA),
            content_hash(&arrays, &moved, SNAPSHOT_SCHEMA)
        );

        let mut carried = captured.clone();
        carried["Suits"]["source"] = serde_json::json!("carried");
        carried["Suits"]["observed_at"] = serde_json::json!(123);
        assert_ne!(
            content_hash(&arrays, &captured, SNAPSHOT_SCHEMA),
            content_hash(&arrays, &carried, SNAPSHOT_SCHEMA)
        );
        assert_ne!(
            content_hash(&arrays, &captured, SNAPSHOT_SCHEMA),
            content_hash(&arrays, &captured, SNAPSHOT_SCHEMA + 1)
        );
    }

    #[test]
    fn manual_request_sets_the_full_scan_trigger() {
        let mut cadence = CadenceState::new(Duration::from_secs(25), Duration::from_secs(20), true);
        cadence.request_manual(false);
        assert_eq!(cadence.take_trigger(), "manual");

        cadence.request_manual(true);
        assert_eq!(cadence.take_trigger(), "manual-rewrite");
    }

    #[test]
    fn cadence_waits_for_the_settle_unless_a_trigger_arrives() {
        let start = Instant::now();
        let mut cadence = CadenceState::new(Duration::from_secs(25), Duration::from_secs(20), true);
        let last = start - Duration::from_secs(60);
        assert!(!cadence.due(start, start, last));

        // A historical (catch-up) marker cannot arm the burst.
        cadence.observe_line("Sys [Info]: Logged in SyntheticTenno", true, start);
        assert!(!cadence.due(start, start, last));

        // A current-session marker bypasses the settle.
        cadence.observe_line("Sys [Info]: Logged in SyntheticTenno", false, start);
        assert!(cadence.due(start, start, last));
        assert_eq!(cadence.take_trigger(), "login");

        // A plain timer still waits out the settle.
        let plain = CadenceState::new(Duration::from_secs(25), Duration::from_secs(20), true);
        assert!(!plain.due(start, start, last));
        assert!(plain.due(
            start + Duration::from_secs(25),
            start,
            start + Duration::from_secs(5)
        ));
    }

    #[test]
    fn a_complete_capture_ends_the_burst_and_failed_scans_retry_fast() {
        let start = Instant::now();
        let mut cadence = CadenceState::new(Duration::from_secs(25), Duration::from_secs(20), true);
        cadence.observe_line("Sys [Info]: Logged in SyntheticTenno", false, start);
        assert_eq!(cadence.take_trigger(), "login");

        cadence.record_scan(true);
        assert_eq!(cadence.interval, FULL_FALLBACK);

        cadence.record_scan(false);
        assert_eq!(cadence.interval, INCOMPLETE_RETRY);

        // A new marker restarts the burst.
        cadence.observe_line("Starting new background region:", false, start);
        assert!(cadence.due(
            start + Duration::from_secs(25),
            start,
            start + Duration::from_secs(5)
        ));

        // Events disabled: no triggers, 20 s/5 s timer.
        let mut silent = CadenceState::new(Duration::from_secs(25), Duration::from_secs(20), false);
        silent.observe_line("Sys [Info]: Logged in SyntheticTenno", false, start);
        assert_eq!(silent.take_trigger(), "timer");
        silent.record_scan(true);
        assert_eq!(silent.interval, Duration::from_secs(20));
    }

    #[test]
    fn pid_loss_and_truncation_clear_a_pending_trigger() {
        let start = Instant::now();
        let mut cadence = CadenceState::new(Duration::from_secs(25), Duration::from_secs(20), true);
        cadence.observe_line("Sys [Info]: Logged in SyntheticTenno", false, start);
        cadence.on_pid_loss();
        assert_eq!(cadence.take_trigger(), "timer");
        assert!(cadence.burst_until.is_none());

        cadence.observe_line("Sys [Info]: Logged in SyntheticTenno", false, start);
        cadence.on_truncation(start);
        assert_eq!(cadence.take_trigger(), "timer");
        assert!(cadence.burst_until.is_some());
    }

    #[test]
    fn a_partial_scan_never_carries_an_old_schema_cache() {
        let old = StatusFile {
            snapshot_schema: SNAPSHOT_SCHEMA - 1,
            game_pid: 7,
            fast_cluster: Some((1, 2)),
            fast_regions: vec![(1, 3)],
            ..StatusFile::default()
        };
        assert_eq!(
            status_cache(false, Some((9, 10)), &[(9, 11)], Some(&old), 7),
            FastCache::default()
        );
        let current = StatusFile {
            snapshot_schema: SNAPSHOT_SCHEMA,
            ..old.clone()
        };
        assert_eq!(
            status_cache(false, Some((9, 10)), &[(9, 11)], Some(&current), 7),
            FastCache {
                cluster: Some((1, 2)),
                region: vec![(1, 3)]
            }
        );
        assert_eq!(
            status_cache(true, Some((9, 10)), &[(9, 11)], Some(&old), 7),
            FastCache {
                cluster: Some((9, 10)),
                region: vec![(9, 11)]
            }
        );
        assert_eq!(
            status_cache(false, Some((9, 10)), &[(9, 11)], None, 7),
            FastCache {
                cluster: Some((9, 10)),
                region: vec![(9, 11)]
            }
        );
    }

    #[test]
    fn persists_and_discards_the_fast_cache_by_pid_and_schema() {
        let status = StatusFile {
            snapshot_schema: SNAPSHOT_SCHEMA,
            game_pid: 7,
            fast_cluster: Some((10, 20)),
            fast_regions: vec![(10, 30)],
            ..StatusFile::default()
        };
        let cache = cache_from_status(7, &status);
        assert_eq!(cache.cluster, Some((10, 20)));
        assert_eq!(cache.region, vec![(10, 30)]);
        assert!(cache_from_status(8, &status).is_empty());
        // A cache written before the current required set cannot be trusted.
        let old = StatusFile {
            snapshot_schema: SNAPSHOT_SCHEMA - 1,
            ..status
        };
        assert!(cache_from_status(7, &old).is_empty());
    }

    #[test]
    fn pid_events_clear_the_session_cache() {
        let now = Instant::now();
        let mut game = None;
        assert_eq!(observe_pid(&mut game, Some(7), now), PidEvent::Appeared(7));
        assert_eq!(observe_pid(&mut game, Some(7), now), PidEvent::None);
        assert_eq!(observe_pid(&mut game, Some(8), now), PidEvent::Replaced(8));
        assert_eq!(observe_pid(&mut game, None, now), PidEvent::Lost(8));

        let mut cache = FastCache {
            cluster: Some((1, 2)),
            region: vec![(1, 3)],
        };
        let mut misses = 4;
        assert!(reset_session(
            PidEvent::Replaced(9),
            &mut cache,
            &mut misses
        ));
        assert!(cache.is_empty());
        assert_eq!(misses, 0);
        assert!(!reset_session(PidEvent::None, &mut cache, &mut misses));
    }

    #[test]
    fn an_array_larger_than_the_window_is_reported_as_too_large() {
        let path = std::env::temp_dir().join(format!(
            "coruframe-toolarge-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        std::io::Write::write_all(&mut file, br#"{"Suits":["#).unwrap();
        let end = EXTRACTION_WINDOW + (1 << 20);
        file.set_len(end).unwrap();
        let mut budget = EXTRACTION_BUDGET;
        assert_eq!(
            extract_array(&file, 0, end, 1, "Suits", &mut budget),
            Err(ExtractMiss::TooLarge)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_failed_status_write_backs_off() {
        let now = Instant::now();
        let mut gate = WriteGate::new();
        assert!(gate.ready(now));
        gate.failed(now);
        assert!(!gate.ready(now + Duration::from_secs(1)));
        assert!(gate.ready(now + WRITE_BACKOFF));
        gate.ok();
        assert!(gate.ready(now + Duration::from_secs(1)));
    }

    #[test]
    fn status_write_reports_a_read_only_cache() {
        let dir = std::env::temp_dir().join(format!("coruframe-ro-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
        let result = write_status_at(&dir.join("inventory-status.json"), &StatusFile::default());
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(result.is_err(), "a read-only cache must not report success");
    }

    #[test]
    fn tier_selection_prefers_the_cluster_and_falls_back_to_full() {
        let cache = FastCache {
            cluster: Some((0x1000, 0x2000)),
            region: vec![(0x1000, 0x9000)],
        };
        assert_eq!(next_tier("timer", true, &cache, 0), ScanTier::Cluster);
        assert_eq!(next_tier("login", true, &cache, 0), ScanTier::Full);
        assert_eq!(next_tier("timer", false, &cache, 0), ScanTier::Full);
        assert_eq!(
            next_tier("timer", true, &cache, FAST_MISSES_BEFORE_FULL),
            ScanTier::Full
        );
        let region_only = FastCache {
            cluster: None,
            region: vec![(0x1000, 0x9000)],
        };
        assert_eq!(next_tier("timer", true, &region_only, 0), ScanTier::Region);
        let empty = FastCache::default();
        assert_eq!(next_tier("timer", true, &empty, 0), ScanTier::Full);
    }

    #[test]
    fn rank_arrays_are_required_even_when_empty() {
        let mut json = serde_json::json!({
            "MiscItems": [], "Suits": [], "LongGuns": [], "Pistols": [],
            "Melee": [], "XPInfo": [], "Sentinels": [], "SentinelWeapons": [],
            "KubrowPets": [], "MechSuits": [], "SpaceSuits": [],
            "SpaceGuns": [], "SpaceMelee": [], "OperatorAmps": []
        });
        let (complete, _, missing) = snapshot_status(json.as_object().unwrap());
        assert!(!complete);
        assert!(missing.contains(&"RawUpgrades".to_string()));
        assert!(missing.contains(&"Upgrades".to_string()));

        // An empty array is a real "no mods" answer and must pass.
        json["RawUpgrades"] = serde_json::json!([]);
        json["Upgrades"] = serde_json::json!([]);
        let (complete, _, missing) = snapshot_status(json.as_object().unwrap());
        assert!(complete);
        assert!(!missing.contains(&"RawUpgrades".to_string()));
        assert!(!missing.contains(&"Upgrades".to_string()));
    }

    #[test]
    fn requires_every_tracked_array_for_a_complete_snapshot() {
        let json = serde_json::json!({
            "MiscItems": [{"ItemType": "/L/Misc"}],
            "Suits": [{"ItemType": "/L/Suit"}],
            "LongGuns": [{"ItemType": "/L/Gun"}],
            "Pistols": [{"ItemType": "/L/Pistol"}],
            "Melee": [{"ItemType": "/L/Melee"}],
            "XPInfo": [{"ItemType": "/L/Suit", "XP": 1}],
            "Sentinels": [],
            "SentinelWeapons": [],
            "KubrowPets": [],
            "MechSuits": [],
            "SpaceSuits": [],
            "SpaceGuns": [],
            "SpaceMelee": [],
            "OperatorAmps": [{"ItemType": "/L/Amp"}],
            "RawUpgrades": [],
            "Upgrades": [{"ItemType": "/L/Mod", "lvl": 5}],
        });
        let (complete, captured, missing) = snapshot_status(json.as_object().unwrap());
        assert!(complete);
        assert!(captured.contains(&"MechSuits".to_string()));
        for key in REQUIRED_KEYS
            .iter()
            .chain(OPTIONAL_KEYS.iter())
            .chain(inventory_keys::RANKS.iter())
        {
            assert!(!missing.contains(&key.to_string()), "{key} missing");
        }

        let core_only = serde_json::json!({
            "MiscItems": [{"ItemType": "/L/Misc"}],
            "Suits": [{"ItemType": "/L/Suit"}],
            "LongGuns": [{"ItemType": "/L/Gun"}],
            "Pistols": [{"ItemType": "/L/Pistol"}],
            "Melee": [{"ItemType": "/L/Melee"}],
            "XPInfo": [{"ItemType": "/L/Suit", "XP": 1}],
        });
        let (complete, _, missing) = snapshot_status(core_only.as_object().unwrap());
        assert!(!complete);
        assert!(missing.contains(&"MechSuits".to_string()));

        let partial = serde_json::json!({
            "MiscItems": [{"ItemType": "/L/Misc"}],
            "Suits": [{"ItemType": "/L/Suit"}],
        });
        let (complete, _, missing) = snapshot_status(partial.as_object().unwrap());
        assert!(!complete);
        for key in ["LongGuns", "Pistols", "Melee", "XPInfo", "OperatorAmps"] {
            assert!(
                missing.contains(&key.to_string()),
                "{key} not in {missing:?}"
            );
        }
        assert!(!missing.contains(&"MiscItems".to_string()));
        assert!(!missing.contains(&"Suits".to_string()));
    }
}
