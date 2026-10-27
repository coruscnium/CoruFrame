//! Corner reward HUD: one layer-shell panel, one block per reward card.
//!
//! `--mock` draws the committed visual fixture. Every live draw uses
//! `--payload`: the probe's typed card payload with a
//! confidence tier per card. The HUD never blocks on the network.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use coruframe_core::art;
use coruframe_core::catalog::{Catalog, ConfidenceTier, Item};
use coruframe_core::fetch;
use coruframe_core::itemdb::{find_reward_component, is_main_blueprint, ComponentRef, ItemDb};
use coruframe_core::ocr_payload::RewardPayload;
use coruframe_core::overlay_config::Corner;
use coruframe_core::price;
use coruframe_core::private_fs;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4_layer_shell::{Edge, KeyboardMode, Layer, LayerShell};
use serde::Deserialize;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const MARGIN: i32 = 24;
const BIG_ART: i32 = 76;
const MINI_ART: i32 = 34;
const HUD_WIDTH: i32 = 460;
const PRICE_INTERVAL: Duration = Duration::from_millis(350);
const PRICE_TIMEOUT: Duration = Duration::from_secs(4);
const PRICE_BUDGET: Duration = Duration::from_secs(12);
const PLATINUM_PNG: &[u8] = include_bytes!("../assets/Platinum.png");
const PART_PLACEHOLDER_PNG: &[u8] = include_bytes!("../assets/PartPlaceholder.png");

#[derive(Deserialize)]
struct Hud {
    platinum: PathBuf,
    blocks: Vec<Block>,
    #[serde(skip)]
    prices: Vec<PriceJob>,
}

#[derive(Clone)]
struct PriceJob {
    // Block index, not job index: payload mode can have unknown cards between
    // priced ones, and a price must never land on the wrong block.
    index: usize,
    part: Option<String>,
    set: Option<String>,
}

#[derive(Deserialize)]
struct Block {
    part: String,
    #[serde(default)]
    part_price: Option<u32>,
    #[serde(default)]
    set: Option<String>,
    #[serde(default)]
    set_price: Option<u32>,
    thumb: PathBuf,
    #[serde(default)]
    parts: Vec<Part>,
    #[serde(default)]
    unknown: bool,
    #[serde(default = "priced_default")]
    priced: bool,
}

fn priced_default() -> bool {
    true
}

#[derive(Deserialize)]
struct Part {
    name: String,
    thumb: PathBuf,
    #[serde(default)]
    owned: Option<u32>,
    #[serde(default)]
    required: Option<u64>,
}

pub fn run(args: &[String]) -> Result<()> {
    let mut mock: Option<PathBuf> = None;
    let mut payload: Option<PathBuf> = None;
    let mut assets: Option<PathBuf> = None;
    let mut seconds = 0u32;
    let mut prices = true;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--mock" => mock = Some(args.next().ok_or("--mock needs a path")?.into()),
            "--payload" => payload = Some(args.next().ok_or("--payload needs a path")?.into()),
            "--assets" => assets = Some(args.next().ok_or("--assets needs a path")?.into()),
            "--seconds" => {
                seconds = args
                    .next()
                    .ok_or("--seconds needs a number")?
                    .parse()
                    .map_err(|_| "--seconds needs a number")?
            }
            "--no-prices" => prices = false,
            other => return Err(format!("unknown option: {other}").into()),
        }
    }
    if mock.is_some() && payload.is_some() {
        return Err("--mock and --payload are exclusive".into());
    }
    let hud = match (mock, payload) {
        (Some(path), None) => mock_hud(&path, assets)?,
        (None, Some(path)) => {
            if assets.is_some() {
                return Err("--assets is only for --mock".into());
            }
            payload_hud(&path, prices)?
        }
        (None, None) => {
            return Err(
                "usage: coruframe hud (--mock PATH [--assets DIR] | --payload PATH) [--no-prices] [--seconds N]"
                    .into(),
            )
        }
        (Some(_), Some(_)) => unreachable!("exclusive modes checked above"),
    };
    draw(&hud, &coruframe_core::overlay_config::load(), seconds)
}

fn mock_hud(path: &Path, assets: Option<PathBuf>) -> Result<Hud> {
    let assets = assets.unwrap_or_else(|| {
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });
    let text = std::fs::read_to_string(path)?;
    let mut hud: Hud = serde_json::from_str(&text)?;
    hud.platinum = assets.join(&hud.platinum);
    for block in &mut hud.blocks {
        block.thumb = assets.join(&block.thumb);
        for part in &mut block.parts {
            part.thumb = assets.join(&part.thumb);
        }
    }
    check_images(&hud)?;
    Ok(hud)
}

fn check_images(hud: &Hud) -> Result<()> {
    let mut paths = vec![&hud.platinum];
    for block in &hud.blocks {
        paths.push(&block.thumb);
        for part in &block.parts {
            paths.push(&part.thumb);
        }
    }
    for path in paths {
        if !path.is_file() {
            return Err(format!("missing HUD image: {}", path.display()).into());
        }
    }
    Ok(())
}

struct Join {
    catalog: Catalog,
    db: ItemDb,
    counts: Option<HashMap<String, u64>>,
    platinum: PathBuf,
    placeholder: PathBuf,
    prices: bool,
}

fn prepare(prices: bool) -> Result<Join> {
    let step = Instant::now();
    let catalog =
        coruframe_core::catalog::load(false).map_err(|error| format!("catalog: {error}"))?;
    eprintln!("hud: catalog {}ms", step.elapsed().as_millis());
    let step = Instant::now();
    let db = ItemDb::load().map_err(|error| format!("itemdb: {error}"))?;
    eprintln!("hud: itemdb {}ms", step.elapsed().as_millis());
    let step = Instant::now();
    let counts = coruframe_core::inventory_keys::load_counts();
    eprintln!("hud: snapshot {}ms", step.elapsed().as_millis());
    Ok(Join {
        catalog,
        db,
        counts,
        platinum: bundled_icon("Platinum.png", PLATINUM_PNG)?,
        placeholder: bundled_icon("PartPlaceholder.png", PART_PLACEHOLDER_PNG)?,
        prices,
    })
}

impl Join {
    fn image(&self, name: &str) -> Option<PathBuf> {
        art::cached(name)
    }

    fn block(&self, reward: &str, index: usize, jobs: &mut Vec<PriceJob>) -> Option<Block> {
        let Some((parent, component)) = find_reward_component(&self.db, reward) else {
            // A matched market item without a set still gets a name and price.
            if let Some(item) = self.catalog.resolve(reward) {
                let image = self
                    .db
                    .resolve(reward)
                    .and_then(|entry| entry.image.clone());
                let thumb = image
                    .as_deref()
                    .and_then(|name| self.image(name))
                    .unwrap_or_else(|| self.placeholder.clone());
                if self.prices {
                    jobs.push(PriceJob {
                        index,
                        part: Some(item.slug.clone()),
                        set: None,
                    });
                }
                eprintln!("hud: {reward} -> no set (name only); price pending");
                return Some(Block {
                    part: reward.to_string(),
                    part_price: None,
                    set: None,
                    set_price: None,
                    thumb,
                    parts: Vec::new(),
                    unknown: false,
                    priced: self.prices,
                });
            }
            if coruframe_core::catalog::untradeable(reward).is_some() {
                eprintln!("hud: {reward} -> untradeable (name only)");
                let thumb = untradeable_image(reward)
                    .and_then(|name| self.image(name))
                    .or_else(|| {
                        self.db
                            .resolve(reward)
                            .and_then(|entry| entry.image.clone())
                            .and_then(|name| self.image(&name))
                    })
                    .unwrap_or_else(|| self.placeholder.clone());
                return Some(Block {
                    part: reward.to_string(),
                    part_price: None,
                    set: None,
                    set_price: None,
                    thumb,
                    parts: Vec::new(),
                    unknown: false,
                    priced: false,
                });
            }
            eprintln!("hud: no match for {reward:?}; skipping");
            return None;
        };
        let set_name = parent.name.clone();
        let main = is_main_blueprint(&set_name, &component.name);
        let art_name = if main {
            parent.image.clone()
        } else {
            component_image(&self.db, component)
        };
        let art = art_name
            .as_deref()
            .and_then(|name| self.image(name))
            .unwrap_or_else(|| self.placeholder.clone());
        let mut parts = Vec::new();
        for part in &parent.components {
            if !part.part || part.unique_name == component.unique_name {
                continue;
            }
            let part_main = is_main_blueprint(&set_name, &part.name);
            let image = if part_main {
                parent.image.clone()
            } else {
                component_image(&self.db, part)
            };
            let thumb = image
                .as_deref()
                .and_then(|name| self.image(name))
                .unwrap_or_else(|| self.placeholder.clone());
            let owned = self.counts.as_ref().map(|counts| {
                counts
                    .get(&part.unique_name)
                    .copied()
                    .unwrap_or(0)
                    .min(u32::MAX as u64) as u32
            });
            parts.push(Part {
                name: part.name.clone(),
                thumb,
                owned,
                required: Some(part.count),
            });
        }
        let part_slug = part_market(&self.catalog, reward, component).map(|item| item.slug.clone());
        let set_slug = self
            .catalog
            .resolve(&format!("{set_name} Set"))
            .map(|item| item.slug.clone());
        if self.prices {
            jobs.push(PriceJob {
                index,
                part: part_slug.clone(),
                set: set_slug.clone(),
            });
        }
        eprintln!(
            "hud: {reward} -> {set_name} | art {art_name:?} | {} mini(s) | part {part_slug:?} set {set_slug:?}",
            parts.len()
        );
        Some(Block {
            part: reward.to_string(),
            part_price: None,
            set: Some(set_name),
            set_price: None,
            thumb: art,
            parts,
            unknown: false,
            priced: self.prices,
        })
    }
}

// The one rule that keeps a wrong price off the screen: no high tier, no price.
fn price_enabled(prices: bool, tier: ConfidenceTier, chosen: bool) -> bool {
    prices && chosen && tier == ConfidenceTier::High
}

// Payload mode: every visible card is drawn, including the ones the probe could
// not identify. Only a high-confidence card gets a price job; everything else
// shows `?`.
fn payload_hud(path: &Path, prices: bool) -> Result<Hud> {
    let started = Instant::now();
    let payload = RewardPayload::load(path)?;
    let join = prepare(prices)?;
    let mut jobs = Vec::new();
    let mut blocks = Vec::new();
    for card in &payload.cards {
        let tier = card.confidence.tier;
        let chosen = card.chosen.as_ref();
        // A high card is joined even with --no-prices: it still gets its name,
        // set and parts, just no price row. Medium and low never reach the join.
        if tier == ConfidenceTier::High {
            let index = blocks.len();
            if let Some(chosen) = chosen {
                if let Some(mut block) = join.block(&chosen.name, index, &mut jobs) {
                    block.priced = price_enabled(prices, tier, true) && block.priced;
                    if block.priced {
                        // The probe's cached quote draws immediately; the
                        // background quote replaces it when it arrives.
                        block.part_price = card.price;
                    }
                    if let (Some(owned), Some(part)) = (card.owned, chosen.part.as_deref()) {
                        if let Some(mini) = block.parts.iter_mut().find(|mini| mini.name == part) {
                            mini.owned = Some(owned);
                        }
                    }
                    blocks.push(block);
                    continue;
                }
            }
        }
        let name = chosen
            .map(|chosen| chosen.name.clone())
            .unwrap_or_else(|| "Unrecognized card".to_string());
        let tier_name = match tier {
            ConfidenceTier::High => "high",
            ConfidenceTier::Medium => "medium",
            ConfidenceTier::Low => "low",
        };
        eprintln!("hud: payload card {} -> {name} ({tier_name})", card.index);
        blocks.push(Block {
            part: name,
            part_price: None,
            set: None,
            set_price: None,
            thumb: join.placeholder.clone(),
            parts: Vec::new(),
            unknown: true,
            priced: false,
        });
    }
    if blocks.is_empty() {
        return Err("payload has no cards".into());
    }
    eprintln!(
        "hud: payload {} card(s) joined in {}ms",
        blocks.len(),
        started.elapsed().as_millis()
    );
    Ok(Hud {
        platinum: join.platinum,
        blocks,
        prices: jobs,
    })
}

// Forma has no market item and its itemdb component art is the generic
// blueprint icon; the game's own Forma asset is the right thumbnail.
fn untradeable_image(name: &str) -> Option<&'static str> {
    name.to_ascii_lowercase()
        .starts_with("forma")
        .then_some("Forma.png")
}

fn component_image(db: &ItemDb, part: &ComponentRef) -> Option<String> {
    db.resolve(&part.unique_name)
        .and_then(|entry| entry.image.clone())
        .or_else(|| generic_image(&part.name))
}

// WFCD's per-part entries carry the in-game component art; this fallback keeps
// the HUD useful when a part is missing from the item DB.
fn generic_image(name: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    let image = if lower.ends_with("systems") {
        "GenericWarframePrimeSystem.png"
    } else if lower.ends_with("chassis") {
        "GenericWarframePrimeChassis.png"
    } else if lower.ends_with("neuroptics") || lower.ends_with("helmet") {
        "GenericWarframePrimeHelmet.png"
    } else if lower.ends_with("barrel") {
        "GenericGunPrimeBarrel.png"
    } else if lower.ends_with("receiver") {
        "GenericGunPrimeReceiver.png"
    } else if lower.ends_with("stock") {
        "GenericGunPrimeStock.png"
    } else if lower.ends_with("handle") || lower.ends_with("hilt") {
        "GenericWeaponPrimeHilt.png"
    } else if lower.ends_with("blade") {
        "GenericWeaponPrimeBlade.png"
    } else {
        return None;
    };
    Some(image.to_string())
}

// Warm exactly the files `Join::block` will look for: the parent art for a main
// blueprint, the component art otherwise, every part mini, and the generic
// fallback. The HUD itself only reads the cache; this is what fills it.
pub(crate) fn warm_images(db: &ItemDb, names: &[String], budget: Duration) {
    let mut wanted = BTreeSet::new();
    for name in names.iter().take(32) {
        collect_images(db, name, &mut wanted);
    }
    if wanted.is_empty() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    for image in wanted {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = art::path(&image);
            let _ = tx.send(());
        });
    }
    drop(tx);
    let deadline = Instant::now() + budget;
    while rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .is_ok()
    {}
}

fn collect_images(db: &ItemDb, name: &str, wanted: &mut BTreeSet<String>) {
    if let Some(image) = untradeable_image(name) {
        wanted.insert(image.to_string());
    }
    if let Some((parent, component)) = find_reward_component(db, name) {
        let main = is_main_blueprint(&parent.name, &component.name);
        let primary = if main {
            parent.image.clone()
        } else {
            component_image(db, component)
        };
        if let Some(image) = primary {
            wanted.insert(image);
        }
        for part in &parent.components {
            if !part.part || part.unique_name == component.unique_name {
                continue;
            }
            let image = if is_main_blueprint(&parent.name, &part.name) {
                parent.image.clone()
            } else {
                component_image(db, part)
            };
            if let Some(image) = image {
                wanted.insert(image);
            }
        }
    } else if let Some(image) = db.resolve(name).and_then(|entry| entry.image.clone()) {
        wanted.insert(image);
    }
    if let Some(image) = generic_image(name) {
        wanted.insert(image);
    }
}

fn part_market<'a>(
    catalog: &'a Catalog,
    reward: &str,
    component: &ComponentRef,
) -> Option<&'a Item> {
    catalog
        .resolve(reward)
        .or_else(|| catalog.resolve(&component.unique_name))
        .or_else(|| {
            component
                .unique_name
                .strip_suffix("Component")
                .and_then(|prefix| catalog.resolve(&format!("{prefix}Blueprint")))
        })
}

fn bundled_icon(name: &str, bytes: &[u8]) -> Result<PathBuf> {
    let dir = fetch::cache_dir().join("hud");
    private_fs::ensure_private_dir(&dir)?;
    let path = dir.join(name);
    if !path.is_file() {
        private_fs::write_private(&path, bytes)?;
    }
    Ok(path)
}

enum PriceUpdate {
    Part(usize, Option<u32>),
    Set(usize, Option<u32>),
}

fn fetch_prices(jobs: Vec<PriceJob>, tx: std::sync::mpsc::Sender<PriceUpdate>) {
    let cached = price::app_quotes();
    let mut parts: Vec<(String, Vec<usize>)> = Vec::new();
    let mut sets: Vec<(String, Vec<usize>)> = Vec::new();
    for job in &jobs {
        if let Some(slug) = &job.part {
            add_slug(&mut parts, slug, job.index);
        }
        if let Some(slug) = &job.set {
            add_slug(&mut sets, slug, job.index);
        }
    }
    for (slug, indices) in &parts {
        if let Some(price) = cached.get(slug) {
            eprintln!("hud: cached part {slug} -> {price}");
            for index in indices {
                let _ = tx.send(PriceUpdate::Part(*index, Some(*price)));
            }
        }
    }
    for (slug, indices) in &sets {
        if let Some(price) = cached.get(slug) {
            eprintln!("hud: cached set {slug} -> {price}");
            for index in indices {
                let _ = tx.send(PriceUpdate::Set(*index, Some(*price)));
            }
        }
    }
    let mut pacer = Pacer::new();
    for (slug, indices) in &parts {
        let price = pacer.quote(slug);
        eprintln!("hud: part price {slug} -> {price:?}");
        for index in indices {
            let _ = tx.send(PriceUpdate::Part(*index, price));
        }
    }
    for (slug, indices) in &sets {
        let price = pacer.quote(slug);
        eprintln!("hud: set price {slug} -> {price:?}");
        for index in indices {
            let _ = tx.send(PriceUpdate::Set(*index, price));
        }
    }
}

fn add_slug(list: &mut Vec<(String, Vec<usize>)>, slug: &str, index: usize) {
    if let Some((_, indices)) = list.iter_mut().find(|(known, _)| known == slug) {
        indices.push(index);
    } else {
        list.push((slug.to_string(), vec![index]));
    }
}

fn price_text(price: Option<u32>) -> String {
    price
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

struct Pacer {
    last: Option<Instant>,
    deadline: Instant,
}

impl Pacer {
    fn new() -> Self {
        Self {
            last: None,
            deadline: Instant::now() + PRICE_BUDGET,
        }
    }

    fn quote(&mut self, slug: &str) -> Option<u32> {
        if Instant::now() >= self.deadline {
            return None;
        }
        if let Some(last) = self.last {
            let elapsed = last.elapsed();
            if elapsed < PRICE_INTERVAL {
                std::thread::sleep(PRICE_INTERVAL - elapsed);
            }
        }
        self.last = Some(Instant::now());
        price::quote_slug(slug, PRICE_TIMEOUT)
            .ok()
            .flatten()
            .and_then(|(platinum, _)| u32::try_from(platinum).ok())
    }
}

fn draw(
    hud: &Hud,
    config: &coruframe_core::overlay_config::OverlayConfig,
    seconds: u32,
) -> Result<()> {
    gtk4::init().map_err(|error| format!("gtk init: {error}"))?;
    if !gtk4_layer_shell::is_supported() {
        return Err("no layer-shell on this compositor; the reward HUD cannot draw".into());
    }
    let display = gtk4::gdk::Display::default().ok_or("no display")?;
    install_css(&display);
    let window = gtk4::Window::new();
    window.add_css_class("hud-window");
    window.set_decorated(false);
    window.init_layer_shell();
    window.set_namespace(Some("coruframe-hud"));
    window.set_layer(Layer::Overlay);
    let monitor = find_monitor(&display, config.monitor.as_deref());
    if let Some(monitor) = &monitor {
        window.set_monitor(Some(monitor));
    }
    let monitor_name = monitor
        .as_ref()
        .and_then(|monitor| monitor.connector())
        .map(|name| name.to_string())
        .unwrap_or_else(|| "primary".to_string());
    anchor(&window, config.corner);
    window.set_keyboard_mode(KeyboardMode::None);
    window.set_exclusive_zone(-1);
    // Content-driven height: four cards need more room than the old
    // fixed 420, and one card should not leave a large empty panel.
    let height = (hud.blocks.len() as i32 * 104 + 24).clamp(140, 900);
    window.set_default_size(HUD_WIDTH, height);
    window.connect_map(|window| {
        if let Some(surface) = window.surface() {
            let region = gtk4::cairo::Region::create();
            surface.set_input_region(Some(&region));
        }
    });
    let (panel, part_labels, set_labels) = build_panel(hud);
    if !hud.prices.is_empty() {
        for label in part_labels.iter().flatten() {
            label.set_text("?");
        }
        for label in set_labels.iter().flatten() {
            label.set_text("?");
        }
        let jobs = hud.prices.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || fetch_prices(jobs, tx));
        glib::timeout_add_local(Duration::from_millis(50), move || {
            while let Ok(update) = rx.try_recv() {
                match update {
                    PriceUpdate::Part(index, price) => {
                        if let Some(Some(label)) = part_labels.get(index) {
                            label.set_text(&price_text(price));
                        }
                    }
                    PriceUpdate::Set(index, price) => {
                        if let Some(Some(label)) = set_labels.get(index) {
                            label.set_text(&price_text(price));
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }
    panel.set_hexpand(true);
    panel.set_vexpand(true);
    eprintln!(
        "hud: {} block(s) at {:?} on {monitor_name}, {HUD_WIDTH}x{height}",
        hud.blocks.len(),
        config.corner
    );
    window.set_child(Some(&panel));
    window.present();

    // Visual sign-off hook: capture this window only, through KWin ScreenShot2.
    // It keeps the preview free of the rest of the desktop and works with the
    // same allowlist entry as the game capture.
    if let Some(path) = std::env::var_os("CORUFRAME_HUD_SHOT") {
        let path = PathBuf::from(path);
        glib::timeout_add_seconds_local(1, move || {
            if let Some(uuid) = hud_window_uuid() {
                match crate::kwinshot::capture(&uuid, &path) {
                    Ok(path) => eprintln!("hud shot: {}", path.display()),
                    Err(error) => eprintln!("hud shot: {error}"),
                }
            } else {
                eprintln!("hud shot: coruframe-hud window not found");
            }
            glib::ControlFlow::Break
        });
    }

    let main_loop = glib::MainLoop::new(None, false);
    if seconds > 0 {
        let quit = main_loop.clone();
        glib::timeout_add_seconds_local(seconds, move || {
            quit.quit();
            glib::ControlFlow::Break
        });
    }
    main_loop.run();
    Ok(())
}

fn install_css(display: &gtk4::gdk::Display) {
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(
        "
        .hud-window { background: transparent; }
        .hud-panel {
            background-color: rgba(10, 11, 12, 0.88);
            border: 1px solid rgba(58, 56, 49, 0.92);
            border-radius: 12px;
            padding: 8px 14px;
        }
        .hud-block { padding: 9px 0; }
        .hud-part { color: #f2eee6; font-size: 14px; font-weight: 700; }
        .hud-price { color: #dcc9a3; font-size: 13px; font-weight: 600; }
        .hud-set { color: #b8b1a4; font-size: 11px; }
        .hud-set-price { color: #b59a68; font-size: 11px; font-weight: 600; }
        .hud-count { color: #6aa987; font-size: 10px; font-weight: 700; }
        .hud-count-none { color: #8e877b; font-size: 10px; }
        .hud-part-unknown { color: #b8b1a4; font-size: 13px; font-weight: 600; }
        .hud-price-unknown { color: #8e877b; font-size: 13px; font-weight: 600; }
        .hud-sep { background-color: rgba(58, 56, 49, 0.7); min-height: 1px; }
        ",
    );
    gtk4::style_context_add_provider_for_display(
        display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn hud_window_uuid() -> Option<String> {
    let output = std::process::Command::new("kdotool")
        .args(["search", "--class", "coruframe", "getwindowid"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().strip_prefix('{')?.strip_suffix('}'))
        .next_back()
        .map(str::to_string)
}

fn anchor(window: &gtk4::Window, corner: Corner) {
    let (vertical, horizontal) = match corner {
        Corner::TopLeft => (Edge::Top, Edge::Left),
        Corner::TopRight => (Edge::Top, Edge::Right),
        Corner::BottomLeft => (Edge::Bottom, Edge::Left),
        Corner::BottomRight => (Edge::Bottom, Edge::Right),
    };
    window.set_anchor(vertical, true);
    window.set_anchor(horizontal, true);
    window.set_margin(vertical, MARGIN);
    window.set_margin(horizontal, MARGIN);
}

fn find_monitor(
    display: &gtk4::gdk::Display,
    connector: Option<&str>,
) -> Option<gtk4::gdk::Monitor> {
    let model = display.monitors();
    let mut first = None;
    for index in 0..model.n_items() {
        let Some(monitor) = model.item(index).and_downcast::<gtk4::gdk::Monitor>() else {
            continue;
        };
        if first.is_none() {
            first = Some(monitor.clone());
        }
        if let Some(wanted) = connector {
            if monitor.connector().map(|name| name.to_string()).as_deref() == Some(wanted) {
                return Some(monitor);
            }
        }
    }
    first
}

fn build_panel(
    hud: &Hud,
) -> (
    gtk4::Box,
    Vec<Option<gtk4::Label>>,
    Vec<Option<gtk4::Label>>,
) {
    let panel = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    panel.add_css_class("hud-panel");
    let mut part_labels = Vec::new();
    let mut set_labels = Vec::new();
    for (index, block) in hud.blocks.iter().enumerate() {
        if index > 0 {
            let separator = gtk4::Separator::new(gtk4::Orientation::Horizontal);
            separator.add_css_class("hud-sep");
            panel.append(&separator);
        }
        let (row, part_label, set_label) = build_block(block, &hud.platinum);
        panel.append(&row);
        part_labels.push(part_label);
        set_labels.push(set_label);
    }
    (panel, part_labels, set_labels)
}

fn build_block(
    block: &Block,
    platinum: &Path,
) -> (gtk4::Box, Option<gtk4::Label>, Option<gtk4::Label>) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    row.add_css_class("hud-block");
    row.append(&picture(&block.thumb, BIG_ART));

    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 1);
    text.set_valign(gtk4::Align::Center);
    text.set_size_request(190, -1);
    let part = gtk4::Label::new(Some(&block.part));
    part.set_xalign(0.0);
    part.set_wrap(true);
    part.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    if block.unknown {
        part.add_css_class("hud-part-unknown");
    } else {
        part.add_css_class("hud-part");
    }
    text.append(&part);

    let mut part_label = None;
    if block.priced {
        let (part_row, label) = price_row(block.part_price, platinum, "hud-price");
        text.append(&part_row);
        part_label = Some(label);
    } else if block.unknown {
        // The question mark says the card has no priced name.
        let label = gtk4::Label::new(Some("?"));
        label.set_xalign(0.0);
        label.add_css_class("hud-price-unknown");
        text.append(&label);
    }

    let mut set_label = None;
    if let Some(set) = &block.set {
        let set_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let name = gtk4::Label::new(Some(set));
        name.set_xalign(0.0);
        name.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        name.set_max_width_chars(18);
        name.add_css_class("hud-set");
        set_row.append(&name);
        if block.priced {
            let (set_price_row, label) = price_row(block.set_price, platinum, "hud-set-price");
            set_row.append(&set_price_row);
            set_label = Some(label);
        }
        text.append(&set_row);
    }
    row.append(&text);

    let minis = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    minis.set_valign(gtk4::Align::Center);
    for part in &block.parts {
        let cell = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        cell.set_tooltip_text(Some(&part.name));
        cell.append(&picture(&part.thumb, MINI_ART));
        let (count, class) = match (part.owned, part.required) {
            (Some(owned), Some(required)) => (format!("×{owned}/{required}"), "hud-count"),
            (Some(owned), None) => (format!("×{owned}"), "hud-count"),
            (None, Some(required)) => (format!("-/{required}"), "hud-count-none"),
            (None, None) => ("-".to_string(), "hud-count-none"),
        };
        let label = gtk4::Label::new(Some(&count));
        label.add_css_class(class);
        cell.append(&label);
        minis.append(&cell);
    }
    row.append(&minis);
    (row, part_label, set_label)
}

fn picture(path: &Path, size: i32) -> gtk4::Picture {
    picture_size(path, size, size)
}

fn picture_size(path: &Path, width: i32, height: i32) -> gtk4::Picture {
    let picture = gtk4::Picture::for_filename(path);
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_can_shrink(true);
    picture.set_size_request(width, height);
    picture
}

fn price_row(price: Option<u32>, platinum: &Path, class: &str) -> (gtk4::Box, gtk4::Label) {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 3);
    row.append(&picture(platinum, 12));
    let label = gtk4::Label::new(Some(
        &price
            .map(|value| value.to_string())
            .unwrap_or_else(|| "-".into()),
    ));
    label.add_css_class(class);
    row.append(&label);
    (row, label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warming_a_main_blueprint_collects_the_parent_art_and_parts() {
        let json = format!(
            r#"{{"version":{},"source":"test","fetched_at":0,"counts":{{}},"items":[
              {{"uniqueName":"/x/AksomatiPrime","name":"Aksomati Prime","category":"Secondary",
                "kind":"secondary","image":"AksomatiPrime.png","components":[
                {{"uniqueName":"/x/Blueprint","name":"Aksomati Prime Blueprint","part":true,"tradable":true}},
                {{"uniqueName":"/x/Barrel","name":"Aksomati Prime Barrel","part":true,"tradable":true}}
              ]}}]}}"#,
            coruframe_core::itemdb::VERSION
        );
        let db = ItemDb::from_json(&json).unwrap();
        let mut wanted = BTreeSet::new();
        collect_images(&db, "Aksomati Prime Blueprint", &mut wanted);
        assert!(wanted.contains("AksomatiPrime.png"), "{wanted:?}");
        assert!(wanted.contains("GenericGunPrimeBarrel.png"), "{wanted:?}");
    }

    #[test]
    fn generic_images_follow_the_component() {
        assert_eq!(
            generic_image("Hildryn Prime Systems").as_deref(),
            Some("GenericWarframePrimeSystem.png")
        );
        assert_eq!(
            generic_image("Masseter Prime Handle").as_deref(),
            Some("GenericWeaponPrimeHilt.png")
        );
        assert_eq!(
            generic_image("Larkspur Prime Barrel").as_deref(),
            Some("GenericGunPrimeBarrel.png")
        );
        assert_eq!(generic_image("Larkspur Prime Blueprint"), None);
    }

    #[test]
    fn only_a_high_confidence_card_can_show_a_price() {
        assert!(price_enabled(true, ConfidenceTier::High, true));
        assert!(!price_enabled(true, ConfidenceTier::Medium, true));
        assert!(!price_enabled(true, ConfidenceTier::Low, true));
        assert!(!price_enabled(true, ConfidenceTier::High, false));
        assert!(!price_enabled(false, ConfidenceTier::High, true));
    }
}
