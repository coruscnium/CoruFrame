use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use coruframe_core::catalog::{Catalog, Item};
use coruframe_core::fetch;
use coruframe_core::price;
use coruframe_core::private_fs;

const REQUEST_INTERVAL: Duration = Duration::from_millis(350);
// Background indexing leaves headroom under the 3 req/s public limit for
// visible rows, the Market tab and a throttle cooldown.
const BACKGROUND_INTERVAL: Duration = Duration::from_millis(500);
// A throttled request pauses background work; explicit Market tab requests
// still go through.
const COOLDOWN_SECS: u64 = 60;
const RECENT_INTERVAL_SECS: u64 = 60;
// Full books are large; keep them on disk for 15 minutes and let the
// Refresh button force a conditional refetch.
const QUOTE_TTL_SECS: u64 = 900;
// Inventory prices move slowly, but the totals cover every owned stack, so the
// whole collection is rechecked once its quotes pass this age. Conditional
// requests (ETag) make an unchanged answer a 304, so the sweep stays cheap.
const INVENTORY_TTL_SECS: u64 = 30 * 60;
const SAVE_INTERVAL: Duration = Duration::from_secs(5);
// Transient network failures get a short backoff; after this the pair is
// treated as terminal so a dead item cannot pin the worker.
const MAX_RETRIES: u8 = 3;
// An empty top-orders answer only means nobody is listed *right now*. Retry it
// on a short cooldown instead of freezing it for the six-hour inventory TTL.
const EMPTY_TTL_SECS: u64 = 30 * 60;
const SWEEP_INTERVAL_SECS: u64 = 5 * 60;
// Subtype-aware records use a different key shape, so the old rank-only file
// must not be read: an Intact quote would otherwise answer a Radiant row.
const QUOTES_FILE: &str = "quotes-v3.json";

#[derive(Clone, Serialize)]
pub struct MarketItemDto {
    pub id: String,
    pub slug: String,
    pub name: String,
    pub icon: Option<String>,
    pub thumb: Option<String>,
    pub ducats: Option<u64>,
    pub vaulted: bool,
    pub tradable: bool,
    pub max_rank: Option<u64>,
    pub max_charges: Option<u64>,
    pub max_amber_stars: Option<u64>,
    pub max_cyan_stars: Option<u64>,
    pub subtypes: Vec<String>,
    pub tags: Vec<String>,
    pub bulk_tradable: bool,
    pub trading_tax: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct QuoteDto {
    pub slug: String,
    pub platinum: Option<u64>,
    pub online: usize,
    pub approx: bool,
    pub updated_at: u64,
    pub history: Vec<u64>,
    pub variants: Vec<price::VariantPrice>,
}

#[derive(Clone, Serialize)]
pub struct MarketFocusDto {
    pub item: MarketItemDto,
    pub quote: QuoteDto,
    pub sell: Vec<price::OrderInfo>,
    pub buy: Vec<price::OrderInfo>,
    pub best_sell: Option<price::OrderInfo>,
}

#[derive(Clone, Serialize)]
pub struct TrendingDto {
    pub item: MarketItemDto,
    pub orders: usize,
    pub quantity: u64,
    pub min_sell: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct MarketStatusDto {
    pub cached: usize,
    pub queued: usize,
    pub last_recent: u64,
    pub last_error: Option<String>,
}

// One variant of one item: rank for mods and arcanes, subtype for relics. A
// relic family is priced per refinement, so the slug alone cannot answer a row.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
struct VariantKey {
    #[serde(default)]
    rank: Option<u64>,
    #[serde(default)]
    subtype: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
struct QuoteRecord {
    platinum: Option<u64>,
    online: usize,
    approx: bool,
    updated_at: u64,
    #[serde(default)]
    history: Vec<u64>,
    #[serde(default)]
    variants: Vec<price::VariantPrice>,
    // Variants whose top orders were already read, including empty ones.
    #[serde(default)]
    checked: Vec<VariantKey>,
    // When each variant was last attempted, empty answers included. This is
    // the short cooldown; `checked` alone would hide it for the whole TTL.
    #[serde(default)]
    attempted: Vec<(VariantKey, u64)>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
enum QueueMode {
    // Full order book, for the Market tab and variant chips.
    Book,
    // Top orders for one variant (empty keys = the item's own price), used to
    // warm the inventory cheaply.
    Top(VariantKey),
}

struct Pacing {
    last: Instant,
    // Set by a 429/509; background work pauses until then while explicit
    // Market tab requests are still allowed through.
    cooldown_until: Option<Instant>,
}

#[derive(Default)]
struct Inner {
    quotes: HashMap<String, QuoteRecord>,
    queue: VecDeque<(String, QueueMode)>,
    queued: HashSet<(String, QueueMode)>,
    trending: Vec<TrendingDto>,
    last_recent: u64,
    last_error: Option<String>,
    last_save: Option<Instant>,
    dirty: bool,
    retries: HashMap<(String, QueueMode), u8>,
    last_sweep: u64,
    // The inventory's sellable pairs, kept so the sweep can recheck the whole
    // collection even when nothing is on screen.
    inventory: Vec<(String, VariantKey)>,
    // Rows the user can actually see; they revalidate instead of trusting the
    // inventory TTL.
    forced: HashSet<(String, VariantKey)>,
}

#[derive(Clone)]
pub struct MarketService {
    inner: Arc<Mutex<Inner>>,
    catalog: Arc<Catalog>,
    app: AppHandle,
    limiter: Arc<Mutex<Pacing>>,
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

// A market icon is a relative path under the asset host, not a URL. Reject
// absolute URLs, traversal, control characters and empty segments.
fn asset_path(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 300 || value.starts_with('/') {
        return None;
    }
    let valid = value.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b'\\')
    });
    valid.then(|| value.to_string())
}

pub(crate) fn item_dto(item: &Item) -> MarketItemDto {
    MarketItemDto {
        id: item.id.clone(),
        slug: item.slug.clone(),
        name: item.name().to_string(),
        icon: asset_path(&item.i18n.en.icon),
        thumb: asset_path(&item.i18n.en.thumb),
        ducats: item.ducats,
        vaulted: item.vaulted.unwrap_or(false),
        tradable: item.tradable.unwrap_or(true),
        max_rank: item.max_rank,
        max_charges: item.max_charges,
        max_amber_stars: item.max_amber_stars,
        max_cyan_stars: item.max_cyan_stars,
        subtypes: item.subtypes.clone().unwrap_or_default(),
        tags: item.tags.clone(),
        bulk_tradable: item.bulk_tradable.unwrap_or(false),
        trading_tax: item.trading_tax,
    }
}

fn quote_dto(slug: &str, record: Option<&QuoteRecord>) -> QuoteDto {
    match record {
        Some(record) => QuoteDto {
            slug: slug.to_string(),
            platinum: record.platinum,
            online: record.online,
            approx: record.approx,
            updated_at: record.updated_at,
            history: record
                .history
                .iter()
                .rev()
                .take(32)
                .rev()
                .copied()
                .collect(),
            variants: record.variants.clone(),
        },
        None => QuoteDto {
            slug: slug.to_string(),
            platinum: None,
            online: 0,
            approx: false,
            updated_at: 0,
            history: Vec::new(),
            variants: Vec::new(),
        },
    }
}

// Only cached records are returned; a slug with nothing cached is left out so
// callers can tell "no price yet" from "price is null".
fn cached_quotes_from(quotes: &HashMap<String, QuoteRecord>, slugs: &[String]) -> Vec<QuoteDto> {
    let mut seen = HashSet::new();
    slugs
        .iter()
        .filter(|slug| seen.insert(slug.as_str()))
        .filter_map(|slug| quotes.get(slug).map(|record| quote_dto(slug, Some(record))))
        .collect()
}

fn http_status(error: &str) -> Option<u16> {
    let rest = error.split_once("curl HTTP ")?.1;
    rest.split([':', ' ']).next()?.parse().ok()
}

// A 4xx means the item or variant no longer exists, so retrying cannot help.
// Everything else (timeout, 5xx, curl transport failure, bad payload) may pass.
fn is_transient(error: &str) -> bool {
    match http_status(error) {
        Some(status) => status == 0 || status == 408 || status == 429 || status >= 500,
        None => true,
    }
}

// Cloudflare answers 429 for the rate limit and 509 for too many connections.
// Either one means the whole client should pause, not just this pair.
fn throttle_status(error: &str) -> bool {
    matches!(http_status(error), Some(429 | 509))
}

fn attempted_within(record: &QuoteRecord, key: &VariantKey, now: u64) -> bool {
    record
        .attempted
        .iter()
        .any(|(at, at_time)| at == key && now.saturating_sub(*at_time) < EMPTY_TTL_SECS)
}

// An unset rank or subtype means "any", so a plain item is covered by its first
// variant while a relic row needs its own refinement.
fn has_variant(record: &QuoteRecord, key: &VariantKey) -> bool {
    record.variants.iter().any(|variant| {
        (key.rank.is_none() || variant.rank == key.rank)
            && (key.subtype.is_none() || variant.subtype == key.subtype)
    })
}

// The record is fresh enough for its TTL and the answer for this mode is
// already here, or the pair was attempted recently and needs no new request.
fn covered(record: &QuoteRecord, mode: &QueueMode, now: u64) -> bool {
    let ttl = match mode {
        QueueMode::Top(_) => INVENTORY_TTL_SECS,
        QueueMode::Book => QUOTE_TTL_SECS,
    };
    let fresh = now.saturating_sub(record.updated_at) < ttl;
    match mode {
        QueueMode::Top(key) => {
            (fresh && has_variant(record, key)) || attempted_within(record, key, now)
        }
        QueueMode::Book => fresh,
    }
}

// A Top job whose variant is not in the cache is missing work and must run
// before the requotes that only refresh prices already held. Book jobs serve
// the Market tab and keep their normal order.
fn needs_quote(quotes: &HashMap<String, QuoteRecord>, key: &(String, QueueMode)) -> bool {
    match &key.1 {
        QueueMode::Top(variant) => quotes
            .get(&key.0)
            .is_none_or(|record| !has_variant(record, variant)),
        QueueMode::Book => false,
    }
}

// Insert without letting requotes jump a missing price. Visible rows pass
// `front` and still take the head of the queue.
fn push_job(inner: &mut Inner, key: (String, QueueMode), front: bool) {
    if front {
        inner.queue.push_front(key);
        return;
    }
    if needs_quote(&inner.quotes, &key) {
        let at = inner
            .queue
            .iter()
            .rposition(|job| needs_quote(&inner.quotes, job))
            .map_or(0, |index| index + 1);
        inner.queue.insert(at, key);
        return;
    }
    inner.queue.push_back(key);
}

// An empty payload can be a temporary market hiccup; `price::drop_top_cache`
// removes it on the next failed attempt, so a startup pass that parses every
// cached file is redundant. Instead, age out order files the player has not
// touched in a week: the cache otherwise grows without bound.
const CACHE_FILE_MAX_AGE_SECS: u64 = 7 * 24 * 60 * 60;
const CACHE_SWEEP_INTERVAL_SECS: u64 = 60 * 60;

fn sweep_old_market_files(dir: &Path, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !(name.starts_with("orders-") || name.starts_with("top-")) || !name.ends_with(".json") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|meta| meta.modified()) else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age.as_secs() > CACHE_FILE_MAX_AGE_SECS {
            let _ = std::fs::remove_file(&path);
        }
    }
}

// The recent feed may only seed a record that has no authoritative price. A
// book or top read is exact, and letting a newer recent sample replace it would
// downgrade the headline while `approx` stayed false, or reset the timestamp
// and delay the next authoritative recheck. Returns the DTO to emit when the
// record changed.
fn seed_from_recent(
    record: &mut QuoteRecord,
    item: &Item,
    group: &[price::RecentOrder],
    now: u64,
) -> Option<QuoteDto> {
    if !record.approx && record.platinum.is_some() {
        return None;
    }
    // The headline must describe the same variant the worker uses, not the
    // cheapest order across every rank.
    let filter = price::default_filter(item);
    let variants = price::recent_variants(group);
    let default = variants
        .iter()
        .find(|variant| filter.matches_variant(variant))
        .map(|variant| (variant.platinum, variant.online));
    // Once a book or rank read exists, keep its variants and fill the gaps so
    // one recent cycle cannot wipe other inventory ranks.
    if record.approx && record.checked.is_empty() {
        record.variants = variants;
    } else {
        for variant in variants {
            if !record
                .variants
                .iter()
                .any(|existing| MarketService::same_variant(existing, &variant))
            {
                record.variants.push(variant);
            }
        }
    }
    record.updated_at = now;
    if let Some((price, online)) = default {
        record.platinum = Some(price);
        record.online = online;
        record.approx = true;
        if record.history.last() != Some(&price) {
            record.history.push(price);
            if record.history.len() > 32 {
                record.history.remove(0);
            }
        }
    }
    Some(quote_dto(&item.slug, Some(record)))
}

impl MarketService {
    pub fn new(app: AppHandle, catalog: Arc<Catalog>) -> Self {
        let inner = Arc::new(Mutex::new(Inner::default()));
        // The v1 cache keys quotes by rank only, so it cannot answer a subtype
        // row; drop it once instead of letting a stale copy sit on disk.
        let _ = std::fs::remove_file(fetch::cache_dir().join("quotes.json"));
        let quotes_path = fetch::cache_dir().join(QUOTES_FILE);
        if let Ok(text) = std::fs::read_to_string(&quotes_path) {
            if let Ok(quotes) = serde_json::from_str::<HashMap<String, QuoteRecord>>(&text) {
                if let Ok(mut inner) = inner.lock() {
                    inner.quotes = quotes;
                }
            }
        }
        // Loaded from disk above; empty payloads from an earlier session are
        // dropped by the worker's age sweep, not by parsing the cache at startup.

        let service = Self {
            inner,
            catalog,
            app,
            limiter: Arc::new(Mutex::new(Pacing {
                last: Instant::now() - BACKGROUND_INTERVAL,
                cooldown_until: None,
            })),
        };
        service.spawn_worker();
        service
    }

    fn spawn_worker(&self) {
        let service = self.clone();
        std::thread::spawn(move || {
            // Stat-only, so a cold start never parses cached payloads; the cost
            // is one dirent pass, not 3k JSON reads.
            sweep_old_market_files(&fetch::cache_dir(), SystemTime::now());
            let mut last_cache_sweep = unix_now();
            loop {
                let now = unix_now();
                if now.saturating_sub(last_cache_sweep) >= CACHE_SWEEP_INTERVAL_SECS {
                    last_cache_sweep = now;
                    sweep_old_market_files(&fetch::cache_dir(), SystemTime::now());
                }
                let needs_recent = {
                    let inner = service.inner.lock().unwrap();
                    now.saturating_sub(inner.last_recent) >= RECENT_INTERVAL_SECS
                };

                if needs_recent {
                    service.wait(BACKGROUND_INTERVAL, true);
                    match price::fetch_recent(Duration::from_secs(30)) {
                        Ok(orders) => service.apply_recent(&orders),
                        Err(error) => {
                            service.set_error(format!("recent orders: {error}"));
                            service.back_off_if_throttled(&error);
                        }
                    }
                    let mut inner = service.inner.lock().unwrap();
                    inner.last_recent = unix_now();
                }

                // Missing prices and stale quotes both get another pass; the
                // queue keeps the missing ones first.
                let needs_sweep = {
                    let mut inner = service.inner.lock().unwrap();
                    let due = now.saturating_sub(inner.last_sweep) >= SWEEP_INTERVAL_SECS;
                    if due {
                        inner.last_sweep = now;
                    }
                    due
                };
                if needs_sweep {
                    service.requeue_inventory();
                }

                let job = {
                    let mut inner = service.inner.lock().unwrap();
                    let job = inner.queue.pop_front();
                    let forced = match &job {
                        Some((slug, QueueMode::Top(key))) => {
                            inner.forced.remove(&(slug.clone(), key.clone()))
                        }
                        _ => false,
                    };
                    if let Some(job) = &job {
                        inner.queued.remove(job);
                    }
                    job.map(|job| (job, forced))
                };

                if let Some(((slug, mode), forced)) = job {
                    let Some(item) = service.catalog.resolve(&slug) else {
                        // A slug the market catalog does not know cannot start
                        // resolving by retrying.
                        service.clear_retry(&slug, &mode);
                        continue;
                    };
                    let background = matches!(&mode, QueueMode::Top(_));
                    service.wait(
                        if background {
                            BACKGROUND_INTERVAL
                        } else {
                            REQUEST_INTERVAL
                        },
                        background,
                    );
                    let failed = match &mode {
                        QueueMode::Top(key) => {
                            // Inventory rows only need their own variant: the
                            // top orders payload is a few KB, not a book.
                            // A visible row revalidates with its ETag instead
                            // of trusting the inventory cache.
                            let max_age = if forced {
                                Duration::ZERO
                            } else {
                                Duration::from_secs(INVENTORY_TTL_SECS)
                            };
                            let fetched = price::fetch_top_orders(
                                item,
                                key.rank,
                                key.subtype.as_deref(),
                                max_age,
                            );
                            let fetched = match fetched {
                                // A throttled answer is served from cache so
                                // the UI keeps its last price; do not apply it
                                // as fresh, just pause the background.
                                Ok(fetched) if fetched.throttled => {
                                    service.back_off(fetched.retry_after);
                                    Err("curl HTTP 429: throttled".to_string())
                                }
                                Ok(fetched) => price::top_variants(&fetched.body),
                                Err(error) => Err(error),
                            };
                            match fetched {
                                Ok(variants) if variants.is_empty() => {
                                    price::drop_top_cache(item, key.rank, key.subtype.as_deref());
                                    service.mark_empty(&slug, key);
                                    None
                                }
                                Ok(variants) => {
                                    service.apply_top(item, key, &variants);
                                    None
                                }
                                Err(error) => Some(error),
                            }
                        }
                        QueueMode::Book => {
                            let filter = price::default_filter(item);
                            match price::fetch_book_variants(
                                item,
                                Duration::from_secs(QUOTE_TTL_SECS),
                                &filter,
                            ) {
                                Ok((book, variants)) => {
                                    service.apply_book(&item.slug, &book, &variants);
                                    None
                                }
                                Err(error) => Some(error),
                            }
                        }
                    };
                    if let Some(error) = failed {
                        service.set_error(format!("{}: {error}", item.slug));
                        service.back_off_if_throttled(&error);
                        if !is_transient(&error) || !service.retry(&slug, &mode) {
                            // Empty results are already marked checked by the
                            // apply step; a 4xx or exhausted retries land here
                            // so a dead pair stops spinning the queue.
                            service.mark_checked(&slug, &mode);
                        }
                    } else {
                        service.clear_retry(&slug, &mode);
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(500));
                }

                service.maybe_save();
            }
        });
    }

    // User actions (Market tab, orders, inventory rows they are looking at)
    // keep the fast pace and are not blocked by a background throttle pause.
    pub(crate) fn wait_explicit(&self) {
        self.wait(REQUEST_INTERVAL, false);
    }

    // Background indexing paces slower and waits out a 429/509 pause.
    pub(crate) fn wait(&self, interval: Duration, honor_cooldown: bool) {
        if honor_cooldown {
            loop {
                let until = self.limiter.lock().unwrap().cooldown_until;
                let Some(until) = until else {
                    break;
                };
                let now = Instant::now();
                if now >= until {
                    let mut pace = self.limiter.lock().unwrap();
                    if pace.cooldown_until == Some(until) {
                        pace.cooldown_until = None;
                    }
                    break;
                }
                std::thread::sleep(until - now);
            }
        }
        let mut pace = self.limiter.lock().unwrap();
        let elapsed = pace.last.elapsed();
        if elapsed < interval {
            std::thread::sleep(interval - elapsed);
        }
        pace.last = Instant::now();
    }

    fn back_off_if_throttled(&self, error: &str) {
        if throttle_status(error) {
            self.back_off(None);
        }
    }

    // Pause background work. `retry_after` is the server's own pause; the
    // default covers a throttle answer that did not send one.
    fn back_off(&self, retry_after: Option<u64>) {
        let secs = retry_after.unwrap_or(COOLDOWN_SECS).max(1);
        let until = Instant::now() + Duration::from_secs(secs);
        let mut pace = self.limiter.lock().unwrap();
        pace.cooldown_until = Some(pace.cooldown_until.map_or(until, |old| old.max(until)));
    }

    fn set_error(&self, error: String) {
        eprintln!("market: {error}");
        if let Ok(mut inner) = self.inner.lock() {
            inner.last_error = Some(error);
        }
    }

    // Back off, then put the pair back at the end of the queue. Returns false
    // when this pair has used up its attempts.
    fn retry(&self, slug: &str, mode: &QueueMode) -> bool {
        let attempts = {
            let mut inner = self.inner.lock().unwrap();
            let count = inner
                .retries
                .entry((slug.to_string(), mode.clone()))
                .or_insert(0);
            *count += 1;
            *count
        };
        if attempts > MAX_RETRIES {
            self.clear_retry(slug, mode);
            return false;
        }
        std::thread::sleep(Duration::from_secs(5 * u64::from(attempts)));
        let mut inner = self.inner.lock().unwrap();
        let key = (slug.to_string(), mode.clone());
        if !inner.queued.contains(&key) {
            inner.queued.insert(key.clone());
            push_job(&mut inner, key, false);
        }
        true
    }

    fn clear_retry(&self, slug: &str, mode: &QueueMode) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.retries.remove(&(slug.to_string(), mode.clone()));
        }
    }

    // Record a terminal outcome so the short cooldown, not a retry loop,
    // decides when this pair is asked for again.
    fn mark_checked(&self, slug: &str, mode: &QueueMode) {
        let mut inner = self.inner.lock().unwrap();
        let record = inner.quotes.entry(slug.to_string()).or_default();
        if let QueueMode::Top(key) = mode {
            if !record.checked.contains(key) {
                record.checked.push(key.clone());
            }
            record.attempted.retain(|(at, _)| at != key);
            record.attempted.push((key.clone(), unix_now()));
        }
        // A book has no variant marker, so a never-priced one also needs a
        // fresh timestamp to keep `enqueue_one` quiet until the book TTL
        // expires.
        if *mode == QueueMode::Book && record.variants.is_empty() && record.platinum.is_none() {
            record.updated_at = unix_now();
        }
        inner.dirty = true;
    }

    // The market answered with no online sellers for this variant. That is not
    // a price, so only the attempt is recorded; the record keeps its freshness
    // and the recent feed can still fill the gap.
    fn mark_empty(&self, slug: &str, key: &VariantKey) {
        let now = unix_now();
        let mut inner = self.inner.lock().unwrap();
        let record = inner.quotes.entry(slug.to_string()).or_default();
        record.attempted.retain(|(at, _)| at != key);
        record.attempted.push((key.clone(), now));
        inner.dirty = true;
    }

    // Recheck the whole inventory once its quotes age out. Pairs whose price
    // is still missing are queued ahead of the requotes (see `push_job`), so a
    // stale cache never delays a missing price.
    fn requeue_inventory(&self) {
        let now = unix_now();
        let pairs = {
            let inner = self.inner.lock().unwrap();
            let mut pairs = Vec::new();
            for (slug, key) in &inner.inventory {
                let Some(record) = inner.quotes.get(slug) else {
                    pairs.push((slug.clone(), key.clone()));
                    continue;
                };
                if has_variant(record, key) {
                    if now.saturating_sub(record.updated_at) >= INVENTORY_TTL_SECS {
                        pairs.push((slug.clone(), key.clone()));
                    }
                    continue;
                }
                if attempted_within(record, key, now) {
                    continue;
                }
                // An item with no ranked variants is priced by its headline,
                // so a rank-0 inventory row is not missing one.
                if key.rank.is_some()
                    && record.platinum.is_some()
                    && record.variants.iter().all(|variant| variant.rank.is_none())
                {
                    continue;
                }
                pairs.push((slug.clone(), key.clone()));
            }
            pairs
        };
        for (slug, key) in pairs {
            self.enqueue_one(slug, QueueMode::Top(key), false);
        }
    }

    fn apply_book(&self, slug: &str, book: &price::Book, variants: &[price::VariantPrice]) {
        let now = unix_now();
        let changed = {
            let mut inner = self.inner.lock().unwrap();
            let record = inner.quotes.entry(slug.to_string()).or_default();
            record.platinum = book.min_sell;
            record.online = book.online;
            record.approx = false;
            record.updated_at = now;
            record.variants = variants.to_vec();
            if let Some(platinum) = book.min_sell {
                if record.history.last() != Some(&platinum) {
                    record.history.push(platinum);
                    if record.history.len() > 32 {
                        record.history.remove(0);
                    }
                }
            }
            let dto = quote_dto(slug, Some(record));
            inner.dirty = true;
            dto
        };
        let _ = self.app.emit("market-quotes", vec![changed]);
    }

    fn same_variant(left: &price::VariantPrice, right: &price::VariantPrice) -> bool {
        left.rank == right.rank
            && left.charges == right.charges
            && left.amber_stars == right.amber_stars
            && left.cyan_stars == right.cyan_stars
            && left.subtype == right.subtype
    }

    // A top-orders response covers one rank. Merge it into the record instead
    // of replacing the variants other inventory rows already priced.
    fn apply_top(&self, item: &Item, key: &VariantKey, variants: &[price::VariantPrice]) {
        let now = unix_now();
        let changed = {
            let mut inner = self.inner.lock().unwrap();
            let record = inner.quotes.entry(item.slug.clone()).or_default();
            record.attempted.retain(|(at, _)| at != key);
            record.attempted.push((key.clone(), now));
            for variant in variants {
                match record
                    .variants
                    .iter_mut()
                    .find(|existing| Self::same_variant(existing, variant))
                {
                    Some(existing) => *existing = variant.clone(),
                    None => record.variants.push(variant.clone()),
                }
            }
            if has_variant(record, key) && !record.checked.contains(key) {
                record.checked.push(key.clone());
            }
            record.variants.sort_by(|left, right| {
                left.rank
                    .cmp(&right.rank)
                    .then(left.amber_stars.cmp(&right.amber_stars))
                    .then(left.cyan_stars.cmp(&right.cyan_stars))
                    .then(left.charges.cmp(&right.charges))
                    .then(left.subtype.cmp(&right.subtype))
            });
            // A top fetch is exact for its ranks, so the recent feed must not
            // overwrite the merged variants with its rougher sample.
            record.approx = false;
            let filter = price::default_filter(item);
            if let Some(variant) = variants
                .iter()
                .find(|variant| filter.matches_variant(variant))
            {
                record.platinum = Some(variant.platinum);
                record.online = variant.online;
                if record.history.last() != Some(&variant.platinum) {
                    record.history.push(variant.platinum);
                    if record.history.len() > 32 {
                        record.history.remove(0);
                    }
                }
            }
            record.updated_at = now;
            let dto = quote_dto(&item.slug, Some(record));
            inner.dirty = true;
            dto
        };
        let _ = self.app.emit("market-quotes", vec![changed]);
    }

    fn apply_recent(&self, orders: &[price::RecentOrder]) {
        let mut grouped: HashMap<String, Vec<price::RecentOrder>> = HashMap::new();
        for order in orders {
            grouped
                .entry(order.item_id.clone())
                .or_default()
                .push(order.clone());
        }

        let now = unix_now();
        let mut updates = Vec::new();
        let mut trending = Vec::new();
        {
            let mut inner = self.inner.lock().unwrap();
            for (item_id, group) in grouped {
                let Some(item) = self.catalog.by_id(&item_id) else {
                    continue;
                };
                let Some(min) = group.iter().map(|order| order.platinum).min() else {
                    continue;
                };
                let quantity: u64 = group.iter().map(|order| order.quantity).sum();
                trending.push(TrendingDto {
                    item: item_dto(item),
                    orders: group.len(),
                    quantity,
                    min_sell: Some(min),
                });
                let record = inner.quotes.entry(item.slug.clone()).or_default();
                if let Some(dto) = seed_from_recent(record, item, &group, now) {
                    inner.dirty = true;
                    updates.push(dto);
                }
            }
            trending.sort_by(|left, right| {
                right
                    .orders
                    .cmp(&left.orders)
                    .then(right.quantity.cmp(&left.quantity))
                    .then(left.min_sell.cmp(&right.min_sell))
            });
            trending.truncate(12);
            inner.trending = trending.clone();
        }
        if !updates.is_empty() {
            let _ = self.app.emit("market-quotes", updates);
        }
        let _ = self.app.emit("market-trending", trending);
    }

    fn maybe_save(&self) {
        let should_save = {
            let mut inner = self.inner.lock().unwrap();
            let due = inner
                .last_save
                .map(|last| last.elapsed() >= SAVE_INTERVAL)
                .unwrap_or(true);
            if !inner.dirty || !due {
                return;
            }
            inner.dirty = false;
            inner.last_save = Some(Instant::now());
            true
        };
        if !should_save {
            return;
        }
        let quotes = {
            let inner = self.inner.lock().unwrap();
            inner.quotes.clone()
        };
        let Ok(json) = serde_json::to_vec(&quotes) else {
            return;
        };
        let path = fetch::cache_dir().join(QUOTES_FILE);
        let _ = private_fs::write_private(&path, &json);
    }

    pub fn enqueue(&self, slugs: impl IntoIterator<Item = String>) {
        for slug in slugs {
            self.enqueue_one(slug, QueueMode::Book, false);
        }
    }

    // Remember the whole sellable set so the sweep can recheck it later, then
    // enqueue it. Missing variants go ahead of stale quotes via `push_job`.
    pub fn warm_inventory(
        &self,
        rows: impl IntoIterator<Item = (String, Option<u64>, Option<String>)>,
    ) {
        let pairs = rows
            .into_iter()
            .map(|(slug, rank, subtype)| (slug, VariantKey { rank, subtype }))
            .collect::<Vec<_>>();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.inventory = pairs.clone();
        }
        for (slug, key) in pairs {
            self.enqueue_one(slug, QueueMode::Top(key), false);
        }
    }

    // The inventory view needs the cached quotes right away, then the queue
    // refreshes them in the background. `refresh` covers the rows on screen:
    // they revalidate immediately instead of waiting out the inventory TTL.
    // Any other price the UI shows (relic rewards) joins the same queue, TTL
    // and cache as the inventory, so a slug asked for twice costs one fetch.
    pub fn warm_prices(
        &self,
        rows: impl IntoIterator<Item = (String, Option<u64>, Option<String>)>,
    ) {
        for (slug, rank, subtype) in rows {
            self.enqueue_one(slug, QueueMode::Top(VariantKey { rank, subtype }), false);
        }
    }

    pub fn inventory_quotes(
        &self,
        rows: &[(String, Option<u64>, Option<String>)],
        refresh: bool,
    ) -> Vec<QuoteDto> {
        let dtos = {
            let inner = self.inner.lock().unwrap();
            rows.iter()
                .map(|(slug, _, _)| quote_dto(slug, inner.quotes.get(slug)))
                .collect()
        };
        for (slug, rank, subtype) in rows {
            let key = VariantKey {
                rank: *rank,
                subtype: subtype.clone(),
            };
            self.enqueue_one_inner(slug.clone(), QueueMode::Top(key), true, refresh);
        }
        dtos
    }

    fn enqueue_one(&self, slug: String, mode: QueueMode, front: bool) {
        self.enqueue_one_inner(slug, mode, front, false);
    }

    fn enqueue_one_inner(&self, slug: String, mode: QueueMode, front: bool, force: bool) {
        let now = unix_now();
        let mut inner = self.inner.lock().unwrap();
        let key = (slug, mode);
        if force {
            if let QueueMode::Top(variant) = &key.1 {
                inner.forced.insert((key.0.clone(), variant.clone()));
            }
        } else if inner
            .quotes
            .get(&key.0)
            .is_some_and(|record| covered(record, &key.1, now))
        {
            return;
        }
        if inner.queued.contains(&key) {
            if !front {
                return;
            }
            inner.queue.retain(|queued| queued != &key);
            inner.queued.remove(&key);
        }
        inner.queued.insert(key.clone());
        push_job(&mut inner, key, front);
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<MarketItemDto> {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let mut items: Vec<&Item> = self
            .catalog
            .all()
            .iter()
            .filter(|item| !item.is_relic() && item.tradable.unwrap_or(true))
            .filter(|item| {
                item.name().to_lowercase().contains(&query)
                    || item.slug.to_lowercase().contains(&query)
            })
            .collect();
        items.sort_by(|left, right| {
            let score = |item: &Item| {
                let name = item.name().to_lowercase();
                if name == query {
                    0
                } else if name.starts_with(&query) {
                    1
                } else {
                    2
                }
            };
            let left_set = left.set_root.unwrap_or(false) || left.name().ends_with(" Set");
            let right_set = right.set_root.unwrap_or(false) || right.name().ends_with(" Set");
            score(left)
                .cmp(&score(right))
                .then(right_set.cmp(&left_set))
                .then_with(|| left.name().cmp(right.name()))
        });
        items.truncate(limit.clamp(1, 60));
        let dtos: Vec<MarketItemDto> = items.iter().map(|item| item_dto(item)).collect();
        self.enqueue(dtos.iter().map(|item| item.slug.clone()));
        dtos
    }

    pub fn items(&self, slugs: &[String]) -> Vec<MarketItemDto> {
        let mut seen = HashSet::new();
        let dtos: Vec<MarketItemDto> = slugs
            .iter()
            .filter(|slug| seen.insert((*slug).clone()))
            .filter_map(|slug| self.catalog.resolve(slug))
            .map(item_dto)
            .collect();
        self.enqueue(dtos.iter().map(|item| item.slug.clone()));
        dtos
    }

    pub fn quotes(&self, slugs: &[String]) -> Vec<QuoteDto> {
        let dtos = {
            let inner = self.inner.lock().unwrap();
            slugs
                .iter()
                .map(|slug| quote_dto(slug, inner.quotes.get(slug)))
                .collect::<Vec<_>>()
        };
        self.enqueue(slugs.iter().cloned());
        dtos
    }

    // A bulk read of what is already on disk. It must not enqueue: a whole
    // inventory would otherwise fire one full-book request per item.
    pub fn cached_quotes(&self, slugs: &[String]) -> Vec<QuoteDto> {
        let inner = self.inner.lock().unwrap();
        cached_quotes_from(&inner.quotes, slugs)
    }

    pub fn focus(
        &self,
        slug: &str,
        filter: Option<price::OrderFilter>,
        force: bool,
    ) -> Result<MarketFocusDto, String> {
        let item = self
            .catalog
            .resolve(slug)
            .ok_or_else(|| format!("unknown market item: {slug}"))?;
        let filter = filter.unwrap_or_else(|| price::default_filter(item));
        let max_age = if force {
            // A zero age makes fetch revalidate the cached book with its ETag.
            Duration::ZERO
        } else {
            Duration::from_secs(QUOTE_TTL_SECS)
        };
        self.wait_explicit();
        let json = price::fetch_orders(item, max_age)
            .map_err(|error| format!("{}: {error}", item.slug))?;
        let variants = price::variants(&json)?;
        let filtered = price::book_filtered(&json, &filter)?;
        // The order lists show every online listing; the filter only decides
        // which variant the headline price and Whisper target describe.
        let all = price::book_all(&json)?;
        self.apply_book(&item.slug, &filtered, &variants);
        let quote = {
            let inner = self.inner.lock().unwrap();
            quote_dto(&item.slug, inner.quotes.get(&item.slug))
        };
        Ok(MarketFocusDto {
            item: item_dto(item),
            quote,
            best_sell: filtered.sell.first().cloned(),
            sell: all.sell,
            buy: all.buy,
        })
    }

    pub fn trending(&self) -> Vec<TrendingDto> {
        self.inner.lock().unwrap().trending.clone()
    }

    pub fn status(&self) -> MarketStatusDto {
        let inner = self.inner.lock().unwrap();
        MarketStatusDto {
            cached: inner.quotes.len(),
            queued: inner.queue.len(),
            last_recent: inner.last_recent,
            last_error: inner.last_error.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn market_asset_paths_stay_relative_and_traversal_free() {
        assert_eq!(
            asset_path("items/images/ash_prime_set.png").as_deref(),
            Some("items/images/ash_prime_set.png")
        );
        assert!(asset_path("").is_none());
        assert!(asset_path("/absolute.png").is_none());
        assert!(asset_path("https://evil.example/x.png").is_none());
        assert!(asset_path("items/../../secret.png").is_none());
        assert!(asset_path("items//double.png").is_none());
        assert!(asset_path("items/line\nbreak.png").is_none());
        assert!(asset_path("items\\win.png").is_none());
    }

    #[test]
    fn old_quote_files_load_without_variants() {
        let record: QuoteRecord = serde_json::from_str(
            r#"{"platinum":3,"online":1,"approx":false,"updated_at":7,"history":[3]}"#,
        )
        .unwrap();
        assert!(record.variants.is_empty());
        assert_eq!(record.platinum, Some(3));
    }

    #[test]
    fn quote_cache_round_trips_variants() {
        let record: QuoteRecord = serde_json::from_str(
            r#"{"platinum":14,"online":2,"approx":false,"updated_at":7,"history":[3,14],
               "variants":[{"rank":10,"charges":null,"amber_stars":null,"cyan_stars":null,
                            "subtype":"regular","platinum":14,"online":2,"quantity":5}]}"#,
        )
        .unwrap();
        let restored: QuoteRecord =
            serde_json::from_str(&serde_json::to_string(&record).unwrap()).unwrap();
        assert_eq!(restored.variants.len(), 1);
        assert_eq!(restored.variants[0].rank, Some(10));
        assert_eq!(restored.variants[0].quantity, 5);
        assert_eq!(
            quote_dto("vitality", Some(&restored)).variants[0].platinum,
            14
        );
    }

    #[test]
    fn missing_prices_go_before_requotes() {
        let mut quotes = HashMap::new();
        let mut record = QuoteRecord::default();
        record.variants.push(price::VariantPrice {
            rank: None,
            charges: None,
            amber_stars: None,
            cyan_stars: None,
            subtype: None,
            platinum: 5,
            online: 1,
            quantity: 1,
        });
        quotes.insert("quoted".to_string(), record);
        let mut inner = Inner {
            quotes,
            ..Default::default()
        };

        push_job(
            &mut inner,
            ("quoted".to_string(), QueueMode::Top(VariantKey::default())),
            false,
        );
        push_job(
            &mut inner,
            ("missing".to_string(), QueueMode::Top(VariantKey::default())),
            false,
        );
        push_job(
            &mut inner,
            ("visible".to_string(), QueueMode::Top(VariantKey::default())),
            true,
        );

        let order = inner
            .queue
            .iter()
            .map(|(slug, _)| slug.as_str())
            .collect::<Vec<_>>();
        assert_eq!(order, ["visible", "missing", "quoted"]);
    }

    #[test]
    fn cached_quotes_read_without_queueing() {
        let mut quotes = HashMap::new();
        let mut record = QuoteRecord {
            platinum: Some(300),
            online: 4,
            updated_at: 9,
            ..Default::default()
        };
        record.variants.push(price::VariantPrice {
            rank: Some(5),
            charges: None,
            amber_stars: None,
            cyan_stars: None,
            subtype: None,
            platinum: 300,
            online: 4,
            quantity: 2,
        });
        quotes.insert("arcane_hot_shot".to_string(), record);

        let mut inner = Inner {
            quotes,
            ..Default::default()
        };
        inner
            .queue
            .push_back(("arcane_hot_shot".to_string(), QueueMode::Book));
        inner
            .queued
            .insert(("arcane_hot_shot".to_string(), QueueMode::Book));

        let dtos = cached_quotes_from(
            &inner.quotes,
            &[
                "arcane_hot_shot".to_string(),
                "arcane_hot_shot".to_string(),
                "not_cached".to_string(),
            ],
        );

        assert_eq!(
            dtos.len(),
            1,
            "uncached slugs are omitted, duplicates collapse"
        );
        assert_eq!(dtos[0].platinum, Some(300));
        assert_eq!(dtos[0].variants[0].rank, Some(5));
        assert_eq!(inner.queue.len(), 1, "a cached read must not add work");
        assert_eq!(inner.queued.len(), 1);
    }

    #[test]
    fn classifies_fetch_failures() {
        assert!(!is_transient("vitality: curl HTTP 404: not found"));
        assert!(!is_transient("vitality: curl HTTP 400: bad request"));
        assert!(is_transient("vitality: curl HTTP 500: server error"));
        assert!(is_transient("vitality: curl HTTP 000: timed out"));
        assert!(is_transient("vitality: curl: failed to connect"));
        assert!(is_transient("vitality: top orders json: expected value"));
    }

    #[test]
    fn recognizes_throttle_responses() {
        assert!(throttle_status(
            "vitality: curl HTTP 429: too many requests"
        ));
        assert!(throttle_status(
            "vitality: curl HTTP 509: too many connections"
        ));
        assert!(!throttle_status("vitality: curl HTTP 404: not found"));
    }

    #[test]
    fn empty_answers_only_cool_down_briefly() {
        let now = 10_000;
        let key = VariantKey {
            rank: Some(3),
            subtype: None,
        };
        let mut record = QuoteRecord {
            updated_at: now - 60,
            platinum: Some(9),
            ..Default::default()
        };
        record.checked.push(key.clone());
        // A checked marker from an empty answer must not hide the rank for the
        // whole six-hour TTL; that is what made rows look permanently unpriced.
        assert!(!covered(&record, &QueueMode::Top(key.clone()), now));

        record.attempted.push((key.clone(), now - 60));
        assert!(covered(&record, &QueueMode::Top(key.clone()), now));

        record.attempted[0].1 = now - EMPTY_TTL_SECS - 1;
        assert!(!covered(&record, &QueueMode::Top(key.clone()), now));

        record.variants.push(price::VariantPrice {
            rank: Some(3),
            charges: None,
            amber_stars: None,
            cyan_stars: None,
            subtype: None,
            platinum: 9,
            online: 1,
            quantity: 1,
        });
        record.attempted.clear();
        assert!(covered(&record, &QueueMode::Top(key), now));
        assert!(!covered(
            &record,
            &QueueMode::Top(VariantKey {
                rank: Some(5),
                subtype: None
            }),
            now
        ));
    }

    #[test]
    fn a_radiant_row_is_not_covered_by_an_intact_quote() {
        let now = 10_000;
        let mut record = QuoteRecord {
            updated_at: now,
            ..Default::default()
        };
        record.variants.push(price::VariantPrice {
            rank: None,
            charges: None,
            amber_stars: None,
            cyan_stars: None,
            subtype: Some("intact".to_string()),
            platinum: 6,
            online: 1,
            quantity: 1,
        });
        assert!(covered(
            &record,
            &QueueMode::Top(VariantKey {
                rank: None,
                subtype: Some("intact".to_string())
            }),
            now
        ));
        assert!(!covered(
            &record,
            &QueueMode::Top(VariantKey {
                rank: None,
                subtype: Some("radiant".to_string())
            }),
            now
        ));
    }

    #[test]
    fn recent_cannot_replace_an_authoritative_price() {
        let catalog = Catalog::from_json(
            r#"{"data":[{"id":"i1","slug":"vitality","gameRef":"/Vitality","tags":["mod"],
               "i18n":{"en":{"name":"Vitality"}}}]}"#,
        )
        .unwrap();
        let item = catalog.by_id("i1").unwrap();
        let fetched_at = 1_000_000;
        let now = fetched_at + QUOTE_TTL_SECS + 60;
        let mut record = QuoteRecord {
            platinum: Some(42),
            online: 3,
            approx: false,
            updated_at: fetched_at,
            ..Default::default()
        };
        let group = vec![price::RecentOrder {
            item_id: "i1".to_string(),
            platinum: 7,
            quantity: 1,
            ingame_name: String::new(),
            created_at: String::new(),
            rank: None,
            charges: None,
            amber_stars: None,
            cyan_stars: None,
            subtype: None,
        }];
        assert!(seed_from_recent(&mut record, item, &group, now).is_none());
        assert_eq!(record.platinum, Some(42));
        assert_eq!(record.online, 3);
        assert!(!record.approx);
        assert_eq!(record.updated_at, fetched_at);
    }

    #[test]
    fn recent_seeds_an_unpriced_record_as_approximate() {
        let catalog = Catalog::from_json(
            r#"{"data":[{"id":"i1","slug":"vitality","gameRef":"/Vitality","tags":["mod"],
               "i18n":{"en":{"name":"Vitality"}}}]}"#,
        )
        .unwrap();
        let item = catalog.by_id("i1").unwrap();
        let mut record = QuoteRecord::default();
        let group = vec![price::RecentOrder {
            item_id: "i1".to_string(),
            platinum: 7,
            quantity: 1,
            ingame_name: String::new(),
            created_at: String::new(),
            rank: None,
            charges: None,
            amber_stars: None,
            cyan_stars: None,
            subtype: None,
        }];
        let dto = seed_from_recent(&mut record, item, &group, 99).unwrap();
        assert_eq!(dto.platinum, Some(7));
        assert!(dto.approx);
        assert_eq!(record.updated_at, 99);
    }

    #[test]
    fn sweeps_order_files_by_age_only() {
        let dir = std::env::temp_dir().join(format!(
            "coruframe-sweep-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("orders-old.json");
        let recent = dir.join("top-new-all-any.json");
        let other = dir.join("items.json");
        for path in [&old, &recent, &other] {
            std::fs::write(path, b"{}").unwrap();
        }
        let now = SystemTime::now();
        let age = |secs: u64| now - Duration::from_secs(secs);
        std::fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(age(CACHE_FILE_MAX_AGE_SECS + 60))
            .unwrap();
        for path in [&recent, &other] {
            std::fs::File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(age(60))
                .unwrap();
        }

        sweep_old_market_files(&dir, now);

        assert!(!old.exists(), "stale order file must be removed");
        assert!(recent.exists(), "fresh top file must survive");
        assert!(other.exists(), "unrelated cache files must survive");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
