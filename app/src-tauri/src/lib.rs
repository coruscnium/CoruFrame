mod account;
mod equipment;
mod hud_probe;
mod inventory;
mod market;
mod presence;
mod riven;
mod trade_watch;
mod wallet;
mod worldstate;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use coruframe_core::catalog::Catalog;
use coruframe_core::relic;

struct AppState {
    catalog: Arc<Catalog>,
    market: market::MarketService,
    itemdb: Arc<equipment::ItemDbCache>,
    itemdb_updating: Arc<std::sync::atomic::AtomicBool>,
    presence: presence::PresenceHandle,
    hud: hud_probe::HudProbe,
}

#[tauri::command]
async fn catalog_cards(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<equipment::CatalogCardDto>, String> {
    let cache = state.itemdb.clone();
    let catalog = state.catalog.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let db = equipment::load(&cache)?;
        Ok(equipment::cards(&db, &catalog))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn itemdb_status(
    state: tauri::State<'_, AppState>,
) -> Result<equipment::ItemDbStatusDto, String> {
    let cache = state.itemdb.clone();
    tauri::async_runtime::spawn_blocking(move || match equipment::load(&cache) {
        Ok(db) => equipment::status(Some(&db), None),
        Err(error) => equipment::status(None, Some(error)),
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
async fn itemdb_update(
    state: tauri::State<'_, AppState>,
    force: Option<bool>,
) -> Result<equipment::ItemDbStatusDto, String> {
    let cache = state.itemdb.clone();
    let updating = state.itemdb_updating.clone();
    tauri::async_runtime::spawn_blocking(move || {
        equipment::update(&cache, &updating, force.unwrap_or(false))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn inventory_items(
    state: tauri::State<'_, AppState>,
) -> Result<inventory::InventorySnapshotDto, String> {
    let cache = state.itemdb.clone();
    let catalog = state.catalog.clone();
    let market = state.market.clone();
    let snapshot = tauri::async_runtime::spawn_blocking(move || {
        let db = equipment::load(&cache)?;
        inventory::load(&db, &catalog)
    })
    .await
    .map_err(|error| error.to_string())??;

    // Warm the price cache for the whole collection in the background. The
    // inventory keeps showing cached prices while these stream in. The worker
    // drains one job per REQUEST_INTERVAL, so a full queue is minutes of work
    // rather than a burst; rows the user is looking at jump the queue.
    if snapshot.linked {
        let mut pairs = Vec::with_capacity(snapshot.items.len() * 2);
        for item in snapshot.items.iter().filter(|item| item.sellable) {
            if let Some(market) = item.market.as_ref() {
                pairs.push((market.slug.clone(), item.rank, item.subtype.clone()));
            }
            if let Some(set) = item.set.as_ref() {
                pairs.push((set.slug.clone(), None, None));
            }
        }
        market.warm_inventory(pairs);
    }

    Ok(snapshot)
}

#[derive(serde::Serialize)]
struct RelicRewardDto {
    name: String,
    slug: Option<String>,
    chance: f64,
    ducats: Option<u64>,
}

#[derive(serde::Serialize)]
struct RelicEntryDto {
    tier: String,
    name: String,
    state: String,
    rewards: Vec<RelicRewardDto>,
}

// The planner owns the inventory snapshot already; it sends the owned relic
// names and gets back only those drop tables. Reward prices are not fetched
// here: they join the market queue and land in the same cache the inventory
// reads, so a part owned in both places is requested once.
#[tauri::command]
async fn relic_entries(
    state: tauri::State<'_, AppState>,
    names: Vec<String>,
) -> Result<Vec<RelicEntryDto>, String> {
    let catalog = state.catalog.clone();
    let market = state.market.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let wanted = names
            .iter()
            .map(|name| relic::key(name))
            .collect::<std::collections::HashSet<_>>();
        let mut prices = Vec::new();
        let entries = relic::load(false)?
            .into_iter()
            .filter(|entry| {
                wanted.contains(&relic::key(&format!(
                    "{} {} {}",
                    entry.tier, entry.name, entry.state
                )))
            })
            .map(|entry| {
                let rewards = entry
                    .rewards
                    .into_iter()
                    .map(|reward| {
                        let item = catalog.resolve(&reward.item_name);
                        let slug = item
                            .filter(|item| item.tradable.unwrap_or(true))
                            .map(|item| item.slug.clone());
                        if let Some(slug) = slug.as_ref() {
                            prices.push((slug.clone(), None, None));
                        }
                        RelicRewardDto {
                            name: reward.item_name,
                            slug,
                            chance: reward.chance,
                            ducats: item.and_then(|item| item.ducats),
                        }
                    })
                    .collect();
                RelicEntryDto {
                    tier: entry.tier,
                    name: entry.name,
                    state: entry.state,
                    rewards,
                }
            })
            .collect();
        market.warm_prices(prices);
        Ok(entries)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(serde::Deserialize)]
struct RankedSlug {
    slug: String,
    rank: Option<u64>,
    #[serde(default)]
    subtype: Option<String>,
}

// The inventory view sends the rows it is rendering so they jump the prefetch
// queue; `refresh` revalidates those rows instead of trusting the long TTL.
#[tauri::command]
fn market_inventory_quotes(
    state: tauri::State<'_, AppState>,
    pairs: Vec<RankedSlug>,
    refresh: Option<bool>,
) -> Vec<market::QuoteDto> {
    let pairs = pairs
        .into_iter()
        .map(|pair| (pair.slug, pair.rank, pair.subtype))
        .collect::<Vec<_>>();
    state
        .market
        .inventory_quotes(&pairs, refresh.unwrap_or(false))
}

#[tauri::command]
fn market_search(
    state: tauri::State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Vec<market::MarketItemDto> {
    state.market.search(&query, limit.unwrap_or(30))
}

#[tauri::command]
fn market_items(
    state: tauri::State<'_, AppState>,
    slugs: Vec<String>,
) -> Vec<market::MarketItemDto> {
    state.market.items(&slugs)
}

#[tauri::command]
fn market_quotes(state: tauri::State<'_, AppState>, slugs: Vec<String>) -> Vec<market::QuoteDto> {
    state.market.quotes(&slugs)
}

// Cached prices only: no request is queued, so a whole inventory can be read
// in one call.
#[tauri::command]
fn market_cached_quotes(
    state: tauri::State<'_, AppState>,
    slugs: Vec<String>,
) -> Vec<market::QuoteDto> {
    state.market.cached_quotes(&slugs)
}

#[tauri::command]
fn market_status(state: tauri::State<'_, AppState>) -> market::MarketStatusDto {
    state.market.status()
}

#[tauri::command]
fn market_trending(state: tauri::State<'_, AppState>) -> Vec<market::TrendingDto> {
    state.market.trending()
}

#[tauri::command]
async fn market_focus(
    state: tauri::State<'_, AppState>,
    slug: String,
    filter: Option<coruframe_core::price::OrderFilter>,
    force: Option<bool>,
) -> Result<market::MarketFocusDto, String> {
    let market = state.market.clone();
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || market.focus(&slug, filter, force))
        .await
        .map_err(|error| error.to_string())?
}

// Never reads the secret: an unloaded account reports "unknown" so the sidebar
// can show "Checking wallet…" without triggering an unlock prompt.
#[tauri::command]
fn market_account_status(force: Option<bool>) -> account::AccountStatusDto {
    account::status(force.unwrap_or(false))
}

// The one-shot background load the frontend starts after the first paint. It is
// single-flight in the backend, so presence and riven share its result.
#[tauri::command]
async fn market_account_load(
    app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<account::AccountStatusDto, String> {
    let force = force.unwrap_or(false);
    let status = tauri::async_runtime::spawn_blocking(move || account::ensure_loaded(force))
        .await
        .map_err(|error| error.to_string())?;
    let _ = app.emit("account-status", status.clone());
    Ok(status)
}

#[tauri::command]
fn inventory_snapshot_available() -> bool {
    coruframe_core::fetch::cache_dir()
        .join("inventory-raw.json")
        .is_file()
}

// The wrapper writes the small status file on every scan. The app reads it
// first and reloads the large raw file only when `content_hash` changes.
#[tauri::command]
fn inventory_status() -> Option<coruframe_core::snapshot_status::StatusFile> {
    let text = std::fs::read_to_string(coruframe_core::snapshot_status::path()).ok()?;
    serde_json::from_str(&text).ok()
}

// The wrapper consumes this file and runs a full scan, bypassing the settle and
// the fast path. The app polls the status file until last_scan_at moves.
#[tauri::command]
fn request_inventory_scan(rewrite: bool) -> Result<(), String> {
    let path = coruframe_core::snapshot_status::scan_request_path();
    let body = if rewrite {
        b"rewrite".as_slice()
    } else {
        b"scan".as_slice()
    };
    coruframe_core::private_fs::write_private(&path, body)
        .map_err(|error| format!("request scan {}: {error}", path.display()))
}

// Fallback for an older wrapper without a status file. The mtime is the time
// the snapshot content last changed; a re-read of the same inventory does not
// touch it.
#[tauri::command]
fn inventory_snapshot_stamp() -> u64 {
    std::fs::metadata(coruframe_core::fetch::cache_dir().join("inventory-raw.json"))
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|age| age.as_secs())
        .unwrap_or(0)
}

#[tauri::command]
fn market_set_presence(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
    reconnect: Option<bool>,
) -> Result<(), String> {
    let parsed = match status.as_deref() {
        None => None,
        Some(value) => Some(
            presence::PresenceStatus::parse(value)
                .ok_or_else(|| format!("unknown presence status {value}"))?,
        ),
    };
    state.presence.set(parsed, reconnect.unwrap_or(false))
}

#[tauri::command]
fn market_presence_status(state: tauri::State<'_, AppState>) -> presence::PresenceDto {
    state.presence.state()
}

#[tauri::command]
async fn market_account_login(
    email: String,
    password: String,
) -> Result<account::AccountStatusDto, String> {
    tauri::async_runtime::spawn_blocking(move || account::login(&email, &password))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn market_account_connect(token: String) -> Result<account::AccountStatusDto, String> {
    tauri::async_runtime::spawn_blocking(move || account::connect(&token))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn market_account_disconnect() -> account::AccountStatusDto {
    account::disconnect()
}

#[tauri::command]
async fn market_my_orders(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<account::MyOrderDto>, String> {
    let catalog = state.catalog.clone();
    tauri::async_runtime::spawn_blocking(move || account::my_orders(&catalog))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn market_order_create(
    state: tauri::State<'_, AppState>,
    input: account::OrderInput,
) -> Result<Option<String>, String> {
    let catalog = state.catalog.clone();
    let market = state.market.clone();
    tauri::async_runtime::spawn_blocking(move || {
        market.wait_explicit();
        account::create(&catalog, &input)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn market_order_update(
    state: tauri::State<'_, AppState>,
    id: String,
    input: account::OrderInput,
) -> Result<(), String> {
    let catalog = state.catalog.clone();
    let market = state.market.clone();
    tauri::async_runtime::spawn_blocking(move || {
        market.wait_explicit();
        account::update(&catalog, &id, &input)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn market_order_close(
    state: tauri::State<'_, AppState>,
    id: String,
    quantity: u64,
) -> Result<(), String> {
    let market = state.market.clone();
    tauri::async_runtime::spawn_blocking(move || {
        market.wait_explicit();
        account::close(&id, quantity)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn market_order_delete(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let market = state.market.clone();
    tauri::async_runtime::spawn_blocking(move || {
        market.wait_explicit();
        account::delete(&id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn riven_inventory() -> Result<Vec<riven::RivenDto>, String> {
    tauri::async_runtime::spawn_blocking(riven::inventory)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn riven_listings() -> Result<Vec<riven::RivenListingDto>, String> {
    tauri::async_runtime::spawn_blocking(riven::listings)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn worldstate(force: Option<bool>) -> Result<worldstate::WorldstateDto, String> {
    let force = force.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || worldstate::load(force))
        .await
        .map_err(|error| error.to_string())?
}

fn checked_url(url: &str) -> Result<url::Url, String> {
    if url.chars().any(char::is_control) {
        return Err("URL contains control characters".into());
    }
    let parsed = url::Url::parse(url).map_err(|error| format!("invalid URL: {error}"))?;
    if parsed.scheme() != "https" {
        return Err("only https URLs can be opened".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URLs with credentials are not allowed".into());
    }
    Ok(parsed)
}

// Steam runs the launch option as a shell command. The CLI is on PATH in the
// Nix package, so the default names it directly; an explicit wrapper override
// is quoted because Steam's parser splits on spaces.
fn launch_option(path: &std::path::Path) -> String {
    let path = path.display().to_string();
    let path = path.replace('\\', "\\\\").replace('"', "\\\"");
    let path = if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path
    };
    format!("{path} launch -- %command%")
}

fn launch_command_text(wrapper: Option<&std::ffi::OsStr>) -> String {
    wrapper.map_or_else(
        || "coruframe launch -- %command%".to_string(),
        |path| launch_option(std::path::Path::new(path)),
    )
}

#[tauri::command]
fn launch_command() -> String {
    launch_command_text(std::env::var_os("CORUFRAME_WRAPPER").as_deref())
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    checked_url(&url)?;
    std::process::Command::new("xdg-open")
        .arg(&url)
        .spawn()
        .map_err(|error| format!("xdg-open: {error}"))?;
    Ok(())
}

// The CLI owns the GTK4 layer-shell probe, so the app asks the installed
// `coruframe` instead of linking a second GTK major version.
fn cli_candidates(
    current: Option<&std::path::Path>,
    configured: Option<std::path::PathBuf>,
    appdir: Option<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = configured {
        paths.push(path);
    }
    if let Some(appdir) = appdir {
        paths.push(appdir.join("usr/bin/coruframe"));
        paths.push(appdir.join("usr/lib/CoruFrame/coruframe"));
    }
    if let Some(executable) = current {
        if let Some(dir) = executable.parent() {
            paths.push(dir.join("coruframe"));
        }
    }
    paths.retain(|path| Some(path.as_path()) != current);
    paths
}

fn run_cli<T: serde::de::DeserializeOwned>(args: &[&str]) -> Result<T, String> {
    let current = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    let configured = std::env::var_os("CORUFRAME_CLI").map(std::path::PathBuf::from);
    let appdir = std::env::var_os("APPDIR").map(std::path::PathBuf::from);
    let mut candidates = cli_candidates(current.as_deref(), configured, appdir);
    candidates.push(std::path::PathBuf::from("coruframe"));
    let mut last = "coruframe: no CLI found (set CORUFRAME_CLI)".to_string();
    for path in candidates {
        // A PATH entry that resolves back to the GUI would start a second
        // window instead of answering.
        let resolved = path.canonicalize().ok();
        if resolved.is_some() && resolved == current {
            continue;
        }
        let output = match std::process::Command::new(&path).args(args).output() {
            Ok(output) => output,
            Err(error) => {
                last = format!("{}: {error}", path.display());
                continue;
            }
        };
        if output.status.success() {
            return serde_json::from_slice(&output.stdout)
                .map_err(|error| format!("{} {}: {error}", path.display(), args.join(" ")));
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        last = if stderr.is_empty() {
            format!("{} exited {}", path.display(), output.status)
        } else {
            clean_cli_error(&stderr)
        };
    }
    Err(last)
}

// The probe has to keep running, so it needs the file, not one command's
// output. Same order as `run_cli`: explicit override, bundled location, then a
// sibling and PATH. Never the GUI itself.
fn resolve_cli() -> Option<std::path::PathBuf> {
    let current = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok());
    let configured = std::env::var_os("CORUFRAME_CLI").map(std::path::PathBuf::from);
    let appdir = std::env::var_os("APPDIR").map(std::path::PathBuf::from);
    let mut candidates = cli_candidates(current.as_deref(), configured, appdir);
    candidates.push(std::path::PathBuf::from("coruframe"));
    for candidate in candidates {
        let path = if candidate.components().count() == 1 {
            let Some(search) = std::env::var_os("PATH") else {
                continue;
            };
            let found = std::env::split_paths(&search)
                .map(|dir| dir.join(&candidate))
                .find(|path| path.is_file());
            let Some(found) = found else { continue };
            found
        } else {
            candidate
        };
        let Ok(resolved) = path.canonicalize() else {
            continue;
        };
        if Some(&resolved) != current.as_ref() && resolved.is_file() {
            return Some(resolved);
        }
    }
    None
}

// `fn main() -> Result` prints the debug form; the UI wants the sentence.
fn clean_cli_error(text: &str) -> String {
    let text = text.trim();
    let text = text.strip_prefix("Error: ").unwrap_or(text);
    text.strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .unwrap_or(text)
        .to_string()
}

fn cli_capabilities() -> Option<coruframe_core::capabilities::Capabilities> {
    run_cli(&["capabilities", "--json"]).ok()
}

fn cli_monitors() -> Vec<coruframe_core::overlay_config::Monitor> {
    run_cli(&["monitors", "--json"]).unwrap_or_default()
}

#[tauri::command]
fn capabilities() -> coruframe_core::capabilities::Capabilities {
    cli_capabilities().unwrap_or_else(coruframe_core::capabilities::detect)
}

#[derive(serde::Serialize)]
struct HudSettings {
    corner: coruframe_core::overlay_config::Corner,
    monitor: Option<String>,
    monitors: Vec<coruframe_core::overlay_config::Monitor>,
}

#[tauri::command]
fn hud_settings() -> HudSettings {
    let config = coruframe_core::overlay_config::load();
    HudSettings {
        corner: config.corner,
        monitor: config.monitor,
        monitors: cli_monitors(),
    }
}

#[tauri::command]
fn set_hud_settings(corner: String, monitor: Option<String>) -> Result<(), String> {
    let corner = coruframe_core::overlay_config::Corner::parse(&corner)
        .ok_or_else(|| format!("unknown corner: {corner}"))?;
    let monitor = monitor
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty());
    if let Some(name) = &monitor {
        if name.len() > 200 || name.chars().any(char::is_control) {
            return Err("invalid monitor connector".into());
        }
    }
    coruframe_core::overlay_config::save(&coruframe_core::overlay_config::OverlayConfig {
        corner,
        monitor,
    })
    .map_err(|error| format!("write overlay.json: {error}"))
}

#[tauri::command]
async fn choose_capture_source() -> Result<coruframe_core::capture_source::Selection, String> {
    // The portal dialog is user-paced, so keep it off the UI thread.
    tauri::async_runtime::spawn_blocking(|| run_cli(&["capture-source", "--json"]))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn hud_probe_status(state: tauri::State<'_, AppState>) -> hud_probe::HudProbeStatus {
    state.hud.status()
}

#[tauri::command]
fn hud_probe_start(state: tauri::State<'_, AppState>) -> Result<hud_probe::HudProbeStatus, String> {
    let caps = cli_capabilities().unwrap_or_else(coruframe_core::capabilities::detect);
    hud_probe::gate(caps.layer_shell, caps.tesseract.as_deref())?;
    let cli = resolve_cli()
        .ok_or_else(|| "coruframe CLI not found; set CORUFRAME_CLI or install it".to_string())?;
    state.hud.start(&cli)?;
    Ok(state.hud.status())
}

#[tauri::command]
fn hud_probe_stop(state: tauri::State<'_, AppState>) -> hud_probe::HudProbeStatus {
    state.hud.stop();
    state.hud.status()
}

#[tauri::command]
fn hud_probe_log() -> String {
    hud_probe::log_tail()
}

// Only the probe log is opened; the webview never passes a path.
#[tauri::command]
fn open_hud_log() -> Result<(), String> {
    std::process::Command::new("xdg-open")
        .arg(hud_probe::log_path())
        .spawn()
        .map_err(|error| format!("xdg-open: {error}"))?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    coruframe_core::capabilities::prepare_wayland();
    if let Err(message) = coruframe_core::capabilities::require_wayland() {
        eprintln!("{message}");
        std::process::exit(1);
    }
    // WebKitGTK loads TLS as a GIO module. A devshell that only exports dconf
    // leaves the webview unable to open any https:// URL while backend curl
    // still works, which silently breaks every item image. Put the Nix store's
    // glib-networking on the module path before GTK initialises.
    {
        let current = std::env::var("GIO_EXTRA_MODULES").unwrap_or_default();
        let has_tls = current
            .split(':')
            .any(|dir| std::path::Path::new(dir).join("libgiognutls.so").exists());
        if !has_tls {
            let mut found = None;
            if let Ok(store) = std::fs::read_dir("/nix/store") {
                for entry in store.flatten() {
                    let name = entry.file_name();
                    let Some(name) = name.to_str() else { continue };
                    if !name.contains("-glib-networking-") {
                        continue;
                    }
                    let modules = entry.path().join("lib/gio/modules");
                    if modules.join("libgiognutls.so").exists() {
                        found = Some(modules);
                        break;
                    }
                }
            }
            if let Some(modules) = found {
                let modules = modules.display();
                if current.is_empty() {
                    std::env::set_var("GIO_EXTRA_MODULES", modules.to_string());
                } else {
                    std::env::set_var("GIO_EXTRA_MODULES", format!("{modules}:{current}"));
                }
            }
        }
    }

    // The webview sandbox stays on. WebKitGTK's Wayland DMA-BUF path can paint a
    // blank window on some drivers; the supported fallback there is the software
    // renderer (sandbox retained), not disabling the sandbox. This opt-in exists
    // only for a driver that cannot run sandboxed at all.
    let unsafe_sandbox = std::env::var_os("CORUFRAME_UNSAFE_WEBKIT_SANDBOX").is_some();
    if unsafe_sandbox {
        eprintln!(
            "webview: WebKit sandbox DISABLED by CORUFRAME_UNSAFE_WEBKIT_SANDBOX; remove it if the window paints sandboxed"
        );
        std::env::set_var("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS", "1");
    }
    let renderer = if unsafe_sandbox {
        "sandbox off"
    } else if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_some() {
        "sandbox on, software renderer"
    } else {
        "sandbox on, DMA-BUF renderer"
    };
    eprintln!("webview: {renderer}");

    // A shell that exports GDK_BACKEND=x11 would otherwise start the webview
    // under XWayland, where this WebKit renders blank on the GPU path and only
    // software rendering works. Prefer the accelerated Wayland path whenever
    // the session offers it.
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        std::env::set_var("GDK_BACKEND", "wayland");
    }

    // GTK finds its GSettings schemas through XDG_DATA_DIRS. A shell can
    // replace that list without the devshell's gsettings-schemas entries,
    // leaving gtk-xft-dpi unset; WebKit then computes a negative
    // devicePixelRatio and lays the page out off-screen.
    if let Some(schemas) = std::env::var_os("GSETTINGS_SCHEMAS_PATH") {
        let mut dirs: Vec<std::path::PathBuf> = std::env::split_paths(&schemas).collect();
        if let Some(existing) = std::env::var_os("XDG_DATA_DIRS") {
            dirs.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(dirs) {
            std::env::set_var("XDG_DATA_DIRS", joined);
        }
    }
    tauri::Builder::default()
        .setup(|app| {
            let catalog = match coruframe_core::catalog::load(false) {
                Ok(catalog) => Arc::new(catalog),
                Err(error) => {
                    eprintln!("catalog: {error}");
                    Arc::new(
                        Catalog::from_json(r#"{"data":[]}"#)
                            .expect("an empty catalog is always valid"),
                    )
                }
            };
            let market = market::MarketService::new(app.handle().clone(), catalog.clone());
            let presence = presence::spawn({
                let handle = app.handle().clone();
                move |state| {
                    let _ = handle.emit("wfm-presence", state);
                }
            });
            trade_watch::spawn(app.handle().clone());
            app.manage(AppState {
                catalog,
                market,
                itemdb: Arc::new(std::sync::Mutex::new(None)),
                itemdb_updating: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                presence,
                hud: hud_probe::HudProbe::new(app.path().resource_dir().ok()),
            });
            // The toggle is remembered across launches. Starting the probe can
            // wait on a GTK capability probe, so keep it off the first paint.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let state = handle.state::<AppState>();
                if !state.hud.enabled() {
                    return;
                }
                let caps = cli_capabilities().unwrap_or_else(coruframe_core::capabilities::detect);
                if hud_probe::gate(caps.layer_shell, caps.tesseract.as_deref()).is_err() {
                    return;
                }
                if let Some(cli) = resolve_cli() {
                    let _ = state.hud.start(&cli);
                }
            });
            // Correct the modes an older build left behind, off the first paint.
            // The wallet load also hardens synchronously before it reads a token.
            std::thread::spawn(|| {
                for skipped in coruframe_core::private_fs::harden_existing_paths() {
                    eprintln!("permissions: {skipped}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            catalog_cards,
            itemdb_status,
            itemdb_update,
            inventory_items,
            relic_entries,
            market_inventory_quotes,
            market_search,
            market_items,
            market_quotes,
            market_cached_quotes,
            market_status,
            market_trending,
            market_focus,
            market_account_status,
            market_account_load,
            inventory_snapshot_available,
            inventory_snapshot_stamp,
            inventory_status,
            request_inventory_scan,
            market_account_login,
            market_account_connect,
            market_account_disconnect,
            market_set_presence,
            market_presence_status,
            market_my_orders,
            market_order_create,
            market_order_update,
            market_order_close,
            market_order_delete,
            riven_inventory,
            riven_listings,
            worldstate,
            launch_command,
            open_url,
            capabilities,
            hud_settings,
            set_hud_settings,
            hud_probe_status,
            hud_probe_start,
            hud_probe_stop,
            hud_probe_log,
            open_hud_log,
            choose_capture_source
        ])
        .build(tauri::generate_context!())
        .expect("error while building CoruFrame")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app.state::<AppState>().hud.stop();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{checked_url, clean_cli_error, cli_candidates, launch_command_text, launch_option};
    use std::path::{Path, PathBuf};

    #[test]
    fn cli_errors_lose_the_debug_wrapper() {
        assert_eq!(
            clean_cli_error("Error: \"portal: cancelled\""),
            "portal: cancelled"
        );
        assert_eq!(clean_cli_error("plain"), "plain");
    }

    #[test]
    fn the_cli_is_preferred_from_config_then_the_appdir_then_a_sibling() {
        let current = Path::new("/opt/app/coruframe-app");
        assert_eq!(
            cli_candidates(
                Some(current),
                Some(PathBuf::from("/opt/cli")),
                Some(PathBuf::from("/tmp/.mount_CoruFr")),
            ),
            vec![
                PathBuf::from("/opt/cli"),
                PathBuf::from("/tmp/.mount_CoruFr/usr/bin/coruframe"),
                PathBuf::from("/tmp/.mount_CoruFr/usr/lib/CoruFrame/coruframe"),
                PathBuf::from("/opt/app/coruframe"),
            ]
        );
        assert!(cli_candidates(Some(Path::new("/x/coruframe")), None, None).is_empty());
    }

    #[test]
    fn launch_command_defaults_to_the_cli_on_path() {
        assert_eq!(launch_command_text(None), "coruframe launch -- %command%");
        assert_eq!(
            launch_command_text(Some(std::ffi::OsStr::new("/opt/My Apps/coruframe"))),
            "\"/opt/My Apps/coruframe\" launch -- %command%"
        );
    }

    #[test]
    fn launch_option_quotes_paths() {
        assert_eq!(
            launch_option(Path::new("/usr/bin/coruframe")),
            "/usr/bin/coruframe launch -- %command%"
        );
        assert_eq!(
            launch_option(Path::new("/opt/My Apps/CoruFrame.AppImage")),
            "\"/opt/My Apps/CoruFrame.AppImage\" launch -- %command%"
        );
        assert_eq!(
            launch_option(Path::new("/opt/a\"b/coruframe")),
            "/opt/a\\\"b/coruframe launch -- %command%"
        );
    }

    #[test]
    fn open_url_accepts_https_only() {
        assert!(checked_url("https://warframe.market/items/ash_prime_set").is_ok());
        assert!(checked_url("https://wiki.warframe.com/w/Ash").is_ok());
        assert!(checked_url("http://warframe.market").is_err());
        assert!(checked_url("javascript:alert(1)").is_err());
        assert!(checked_url("https://user:pass@warframe.market").is_err());
        assert!(checked_url("https://warframe.market/\n--help").is_err());
        assert!(checked_url("file:///etc/passwd").is_err());
    }
}
