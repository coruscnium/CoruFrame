use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::catalog::Item;
use crate::fetch;

const ORDERS_MAX_AGE: Duration = Duration::from_secs(60);
pub const RECENT_ORDERS_URL: &str = "https://api.warframe.market/v2/orders/recent";

#[derive(Debug, Deserialize)]
struct OrdersDoc {
    data: Vec<Order>,
}

#[derive(Debug, Deserialize)]
struct TopDoc {
    data: TopOrders,
}

#[derive(Debug, Deserialize)]
struct TopOrders {
    #[serde(default)]
    sell: Vec<Order>,
}

#[derive(Debug, Deserialize, Clone)]
struct Order {
    #[serde(rename = "type")]
    kind: String,
    platinum: u64,
    #[serde(default)]
    quantity: u64,
    #[serde(default = "one", rename = "perTrade")]
    per_trade: u64,
    #[serde(default)]
    visible: bool,
    #[serde(default, rename = "itemId")]
    item_id: Option<String>,
    #[serde(default, rename = "createdAt")]
    created_at: Option<String>,
    #[serde(default)]
    rank: Option<u64>,
    #[serde(default)]
    charges: Option<u64>,
    #[serde(default, rename = "amberStars")]
    amber_stars: Option<u64>,
    #[serde(default, rename = "cyanStars")]
    cyan_stars: Option<u64>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    user: User,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct User {
    #[serde(default)]
    status: String,
    #[serde(default, rename = "ingameName")]
    ingame_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderInfo {
    pub platinum: u64,
    pub unit_price: u64,
    pub quantity: u64,
    pub per_trade: u64,
    pub ingame_name: String,
    pub status: String,
    pub rank: Option<u64>,
    pub charges: Option<u64>,
    pub amber_stars: Option<u64>,
    pub cyan_stars: Option<u64>,
    pub subtype: Option<String>,
}

// Market orders carry the variant attributes. These are not cosmetic: an R5
// arcane and an R0 arcane are different prices, as are a 3-star and a bare
// Ayatan sculpture.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OrderFilter {
    #[serde(default)]
    pub rank: Option<u64>,
    #[serde(default)]
    pub charges: Option<u64>,
    #[serde(default)]
    pub amber_stars: Option<u64>,
    #[serde(default)]
    pub cyan_stars: Option<u64>,
    #[serde(default)]
    pub subtype: Option<String>,
}

impl OrderFilter {
    fn matches(&self, order: &Order) -> bool {
        self.rank.is_none_or(|value| order.rank == Some(value))
            && self
                .charges
                .is_none_or(|value| order.charges == Some(value))
            && self
                .amber_stars
                .is_none_or(|value| order.amber_stars == Some(value))
            && self
                .cyan_stars
                .is_none_or(|value| order.cyan_stars == Some(value))
            && self
                .subtype
                .as_ref()
                .is_none_or(|value| order.subtype.as_ref() == Some(value))
    }

    pub fn matches_variant(&self, variant: &VariantPrice) -> bool {
        self.rank.is_none_or(|value| variant.rank == Some(value))
            && self
                .charges
                .is_none_or(|value| variant.charges == Some(value))
            && self
                .amber_stars
                .is_none_or(|value| variant.amber_stars == Some(value))
            && self
                .cyan_stars
                .is_none_or(|value| variant.cyan_stars == Some(value))
            && self
                .subtype
                .as_ref()
                .is_none_or(|value| variant.subtype.as_ref() == Some(value))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Book {
    pub min_sell: Option<u64>,
    pub online: usize,
    pub sell: Vec<OrderInfo>,
    pub buy: Vec<OrderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VariantPrice {
    pub rank: Option<u64>,
    pub charges: Option<u64>,
    pub amber_stars: Option<u64>,
    pub cyan_stars: Option<u64>,
    pub subtype: Option<String>,
    pub platinum: u64,
    pub online: usize,
    pub quantity: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentOrder {
    pub item_id: String,
    pub platinum: u64,
    pub quantity: u64,
    pub ingame_name: String,
    pub created_at: String,
    pub rank: Option<u64>,
    pub charges: Option<u64>,
    pub amber_stars: Option<u64>,
    pub cyan_stars: Option<u64>,
    pub subtype: Option<String>,
}

pub fn orders_url(slug: &str) -> String {
    format!("https://api.warframe.market/v2/orders/item/{slug}")
}

fn one() -> u64 {
    1
}

fn parse(json: &str) -> Result<Vec<Order>, String> {
    let doc: OrdersDoc =
        serde_json::from_str(json).map_err(|error| format!("orders json: {error}"))?;
    Ok(doc.data)
}

fn is_online(order: &Order) -> bool {
    order.visible && order.user.status != "offline"
}

// WFM stores the price for one trade, so the unit price is the per-trade price
// divided by the stack size. The order book sorts and compares unit prices;
// the order row still shows the raw per-trade price.
fn unit_price(order: &Order) -> u64 {
    order.platinum / order.per_trade.max(1)
}

// Lowest visible sell order from a seller who is currently online, plus how many
// such sellers exist. Offline sellers are excluded: their orders are stale.
pub fn min_sell(json: &str) -> Result<Option<(u64, usize)>, String> {
    let mut best: Option<u64> = None;
    let mut online = 0;
    for order in parse(json)? {
        if order.kind != "sell" || !is_online(&order) {
            continue;
        }
        online += 1;
        best = Some(best.map_or(unit_price(&order), |current| {
            current.min(unit_price(&order))
        }));
    }
    Ok(best.map(|platinum| (platinum, online)))
}

// Full order book for the focused item: the cheapest visible online sells and
// the best visible online buys. The UI shows the first few of each.
pub fn book(json: &str) -> Result<Book, String> {
    book_filtered(json, &OrderFilter::default())
}

pub fn book_filtered(json: &str, filter: &OrderFilter) -> Result<Book, String> {
    build_book(json, Some(filter), 20)
}

// The item page lists every online order, whatever its rank or variant; the
// rank dropdown only decides which price is the headline.
pub fn book_all(json: &str) -> Result<Book, String> {
    build_book(json, None, usize::MAX)
}

fn build_book(json: &str, filter: Option<&OrderFilter>, limit: usize) -> Result<Book, String> {
    let mut sell = Vec::new();
    let mut buy = Vec::new();
    let mut online = 0;
    for order in parse(json)? {
        if !is_online(&order) || filter.is_some_and(|filter| !filter.matches(&order)) {
            continue;
        }
        let info = OrderInfo {
            platinum: order.platinum,
            unit_price: unit_price(&order),
            quantity: order.quantity,
            per_trade: order.per_trade,
            ingame_name: order.user.ingame_name,
            status: order.user.status,
            rank: order.rank,
            charges: order.charges,
            amber_stars: order.amber_stars,
            cyan_stars: order.cyan_stars,
            subtype: order.subtype,
        };
        match order.kind.as_str() {
            "sell" => {
                online += 1;
                sell.push(info);
            }
            "buy" => buy.push(info),
            _ => {}
        }
    }
    sell.sort_by_key(|order| order.unit_price);
    buy.sort_by_key(|order| std::cmp::Reverse(order.unit_price));
    sell.truncate(limit);
    buy.truncate(limit);
    let min_sell = sell.first().map(|order| order.unit_price);
    Ok(Book {
        min_sell,
        online,
        sell,
        buy,
    })
}

// Every sell variant of an item, cheapest first within the variant. The card
// chips let the user pick a rank or star count without another fetch.
pub fn variants(json: &str) -> Result<Vec<VariantPrice>, String> {
    Ok(group_variants(parse(json)?.iter()))
}

fn group_variants<'a>(orders: impl Iterator<Item = &'a Order>) -> Vec<VariantPrice> {
    type Key = (
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<String>,
    );
    let mut grouped: HashMap<Key, VariantPrice> = HashMap::new();
    for order in orders {
        if order.kind != "sell" || !is_online(order) {
            continue;
        }
        let key = (
            order.rank,
            order.charges,
            order.amber_stars,
            order.cyan_stars,
            order.subtype.clone(),
        );
        grouped
            .entry(key)
            .and_modify(|variant| {
                variant.platinum = variant.platinum.min(if order.per_trade > 1 {
                    order.platinum / order.per_trade
                } else {
                    order.platinum
                });
                variant.online += 1;
                variant.quantity += order.quantity;
            })
            .or_insert(VariantPrice {
                rank: order.rank,
                charges: order.charges,
                amber_stars: order.amber_stars,
                cyan_stars: order.cyan_stars,
                subtype: order.subtype.clone(),
                platinum: if order.per_trade > 1 {
                    order.platinum / order.per_trade
                } else {
                    order.platinum
                },
                online: 1,
                quantity: order.quantity,
            });
    }
    let mut variants: Vec<VariantPrice> = grouped.into_values().collect();
    variants.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then(left.amber_stars.cmp(&right.amber_stars))
            .then(left.cyan_stars.cmp(&right.cyan_stars))
            .then(left.charges.cmp(&right.charges))
            .then(left.subtype.cmp(&right.subtype))
    });
    variants
}

// The card, the rail and the worker must agree on which variant the headline
// price describes; without this an R0 mod undercuts its max-rank price.
pub fn default_filter(item: &Item) -> OrderFilter {
    OrderFilter {
        rank: item.max_rank,
        charges: item.max_charges,
        amber_stars: item.max_amber_stars,
        cyan_stars: item.max_cyan_stars,
        subtype: item
            .subtypes
            .as_ref()
            .and_then(|values| values.first().cloned()),
    }
}

// The recent feed is a broad, cheap seed: up to 500 visible online sell orders
// created in the last four hours. It is not authoritative for an item's lowest
// order, so a focused fetch corrects it.
pub fn recent(json: &str) -> Result<Vec<RecentOrder>, String> {
    Ok(parse(json)?
        .into_iter()
        .filter(|order| order.kind == "sell" && is_online(order))
        .filter_map(|order| {
            Some(RecentOrder {
                item_id: order.item_id?,
                platinum: if order.per_trade > 1 {
                    order.platinum / order.per_trade
                } else {
                    order.platinum
                },
                quantity: order.quantity,
                ingame_name: order.user.ingame_name,
                created_at: order.created_at.unwrap_or_default(),
                rank: order.rank,
                charges: order.charges,
                amber_stars: order.amber_stars,
                cyan_stars: order.cyan_stars,
                subtype: order.subtype,
            })
        })
        .collect())
}

// The recent feed is one request for hundreds of orders. Grouping it by variant
// gives the inventory an approximate per-rank price before any book is fetched.
pub fn recent_variants(orders: &[RecentOrder]) -> Vec<VariantPrice> {
    type Key = (
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<u64>,
        Option<String>,
    );
    let mut grouped: HashMap<Key, VariantPrice> = HashMap::new();
    for order in orders {
        let key = (
            order.rank,
            order.charges,
            order.amber_stars,
            order.cyan_stars,
            order.subtype.clone(),
        );
        grouped
            .entry(key)
            .and_modify(|variant| {
                variant.platinum = variant.platinum.min(order.platinum);
                variant.online += 1;
                variant.quantity += order.quantity;
            })
            .or_insert(VariantPrice {
                rank: order.rank,
                charges: order.charges,
                amber_stars: order.amber_stars,
                cyan_stars: order.cyan_stars,
                subtype: order.subtype.clone(),
                platinum: order.platinum,
                online: 1,
                quantity: order.quantity,
            });
    }
    let mut variants: Vec<VariantPrice> = grouped.into_values().collect();
    variants.sort_by(|left, right| {
        left.rank
            .cmp(&right.rank)
            .then(left.amber_stars.cmp(&right.amber_stars))
            .then(left.cyan_stars.cmp(&right.cyan_stars))
            .then(left.charges.cmp(&right.charges))
            .then(left.subtype.cmp(&right.subtype))
    });
    variants
}

pub fn fetch_book(item: &Item, max_age: Duration) -> Result<Book, String> {
    fetch_book_filtered(item, max_age, &OrderFilter::default())
}

// One fetch feeds both the filtered book and the variant chips.
pub fn fetch_orders(item: &Item, max_age: Duration) -> Result<String, String> {
    fetch::fetch(
        &orders_url(&item.slug),
        &format!("orders-{}.json", item.slug),
        max_age,
    )
}

pub fn fetch_book_filtered(
    item: &Item,
    max_age: Duration,
    filter: &OrderFilter,
) -> Result<Book, String> {
    book_filtered(&fetch_orders(item, max_age)?, filter)
}

// One network call for the card headline and its variant chips.
pub fn fetch_book_variants(
    item: &Item,
    max_age: Duration,
    filter: &OrderFilter,
) -> Result<(Book, Vec<VariantPrice>), String> {
    let json = fetch_orders(item, max_age)?;
    Ok((book_filtered(&json, filter)?, variants(&json)?))
}

pub fn fetch_recent(max_age: Duration) -> Result<Vec<RecentOrder>, String> {
    let json = fetch::fetch(RECENT_ORDERS_URL, "orders-recent.json", max_age)?;
    recent(&json)
}

// Top orders are the five cheapest online sells (and five buys) for one
// variant: a few KB instead of the hundreds of KB a full book costs, which is
// what makes background prefetching the whole inventory affordable.
fn valid_subtype(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn top_cache_name(item: &Item, rank: Option<u64>, subtype: Option<&str>) -> String {
    let rank = rank.map_or_else(|| "all".to_string(), |rank| rank.to_string());
    let subtype = subtype
        .filter(|value| valid_subtype(value))
        .unwrap_or("any");
    format!("top-{}-{rank}-{subtype}.json", item.slug)
}

fn top_orders_url(slug: &str, rank: Option<u64>, subtype: Option<&str>) -> String {
    let mut params = Vec::with_capacity(2);
    if let Some(rank) = rank {
        params.push(format!("rank={rank}"));
    }
    if let Some(subtype) = subtype.filter(|value| valid_subtype(value)) {
        params.push(format!("subtype={subtype}"));
    }
    let base = format!("https://api.warframe.market/v2/orders/item/{slug}/top");
    if params.is_empty() {
        base
    } else {
        format!("{base}?{}", params.join("&"))
    }
}

// An empty payload is not worth keeping: the market may answer empty during a
// hiccup, and serving that for six hours looks like "nobody sells this".
pub fn drop_top_cache(item: &Item, rank: Option<u64>, subtype: Option<&str>) {
    if subtype.is_some_and(|value| !valid_subtype(value)) {
        return;
    }
    let name = top_cache_name(item, rank, subtype);
    let _ = std::fs::remove_file(fetch::cache_dir().join(name));
}

pub fn fetch_top_orders(
    item: &Item,
    rank: Option<u64>,
    subtype: Option<&str>,
    max_age: Duration,
) -> Result<fetch::Fetched, String> {
    if let Some(value) = subtype {
        if !valid_subtype(value) {
            return Err(format!("invalid subtype {value:?} for {}", item.slug));
        }
        if item
            .subtypes
            .as_ref()
            .is_some_and(|subtypes| !subtypes.iter().any(|known| known == value))
        {
            return Err(format!("unknown subtype {value:?} for {}", item.slug));
        }
    }
    let url = top_orders_url(&item.slug, rank, subtype);
    fetch::fetch_with_meta(&url, &top_cache_name(item, rank, subtype), max_age)
}

pub fn top_variants(json: &str) -> Result<Vec<VariantPrice>, String> {
    let doc: TopDoc =
        serde_json::from_str(json).map_err(|error| format!("top orders json: {error}"))?;
    Ok(group_variants(doc.data.sell.iter()))
}

pub fn quote(item: &Item) -> Result<Option<(u64, usize)>, String> {
    let json = fetch::fetch(
        &orders_url(&item.slug),
        &format!("orders-{}.json", item.slug),
        ORDERS_MAX_AGE,
    )?;
    min_sell(&json)
}

// The reward overlay has a 20 s lifetime, so its reads bound the network stall
// instead of inheriting the generic curl timeout.
pub fn quote_slug(slug: &str, timeout: Duration) -> Result<Option<(u64, usize)>, String> {
    let json = fetch::fetch_with_meta_timeout(
        &orders_url(slug),
        &format!("orders-{slug}.json"),
        ORDERS_MAX_AGE,
        timeout,
    )?
    .body;
    min_sell(&json)
}

pub fn quote_within(item: &Item, timeout: Duration) -> Result<Option<(u64, usize)>, String> {
    quote_slug(&item.slug, timeout)
}

#[derive(Deserialize)]
struct CachedQuote {
    #[serde(default)]
    platinum: Option<u64>,
}

// First-paint prices written by the app (quotes-v3.json). A missing or corrupt
// file is an empty cache, never an error: live quotes still fill in.
pub fn app_quotes() -> HashMap<String, u32> {
    let path = fetch::cache_dir().join("quotes-v3.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let Ok(quotes) = serde_json::from_str::<HashMap<String, CachedQuote>>(&text) else {
        return HashMap::new();
    };
    quotes
        .into_iter()
        .filter_map(|(slug, quote)| {
            quote
                .platinum
                .filter(|price| *price > 0)
                .and_then(|price| u32::try_from(price).ok())
                .map(|price| (slug, price))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDERS: &str = r#"{"apiVersion":"0.25.0","data":[
      {"type":"sell","platinum":40,"quantity":1,"visible":true,"user":{"status":"offline"}},
      {"type":"sell","platinum":35,"quantity":1,"visible":true,"user":{"status":"ingame"}},
      {"type":"sell","platinum":30,"quantity":1,"visible":true,"user":{"status":"online"}},
      {"type":"sell","platinum":5,"quantity":1,"visible":false,"user":{"status":"ingame"}},
      {"type":"buy","platinum":1,"quantity":1,"visible":true,"user":{"status":"ingame"}}
    ]}"#;

    #[test]
    fn ignores_offline_hidden_and_buy_orders() {
        assert_eq!(min_sell(ORDERS).unwrap(), Some((30, 2)));
    }

    #[test]
    fn keeps_per_trade() {
        let json = r#"{"data":[{"type":"sell","platinum":20,"quantity":12,"perTrade":6,"visible":true,"user":{"status":"online","ingameName":"a"}}]}"#;
        let book = book_all(json).unwrap();
        assert_eq!(book.sell[0].platinum, 20);
        assert_eq!(book.sell[0].per_trade, 6);
        assert_eq!(book.sell[0].unit_price, 3);
    }

    #[test]
    fn reports_no_online_sellers() {
        let json = r#"{"data":[{"type":"sell","platinum":5,"quantity":1,"visible":true,"user":{"status":"offline"}}]}"#;
        assert_eq!(min_sell(json).unwrap(), None);
    }
}

#[cfg(test)]
mod book_tests {
    use super::*;

    const BOOK: &str = r#"{"apiVersion":"0.25.0","data":[
      {"type":"sell","platinum":40,"quantity":1,"visible":true,"itemId":"i1","user":{"status":"offline","ingameName":"off"}},
      {"type":"sell","platinum":35,"quantity":2,"visible":true,"itemId":"i1","user":{"status":"ingame","ingameName":"alpha"}},
      {"type":"sell","platinum":30,"quantity":1,"visible":true,"itemId":"i1","user":{"status":"online","ingameName":"bravo"}},
      {"type":"buy","platinum":20,"quantity":3,"visible":true,"itemId":"i1","user":{"status":"online","ingameName":"charlie"}},
      {"type":"buy","platinum":25,"quantity":1,"visible":true,"itemId":"i1","user":{"status":"ingame","ingameName":"delta"}},
      {"type":"sell","platinum":5,"quantity":1,"visible":false,"itemId":"i1","user":{"status":"ingame","ingameName":"hidden"}}
    ]}"#;

    #[test]
    fn book_keeps_online_visible_orders_and_sorts() {
        let book = book(BOOK).unwrap();
        assert_eq!(book.min_sell, Some(30));
        assert_eq!(book.online, 2);
        assert_eq!(book.sell.len(), 2);
        assert_eq!(book.sell[0].ingame_name, "bravo");
        assert_eq!(book.sell[1].ingame_name, "alpha");
        assert_eq!(book.buy[0].ingame_name, "delta");
        assert_eq!(book.buy[1].ingame_name, "charlie");
    }

    #[test]
    fn all_orders_ignore_the_rank_filter() {
        let json = r#"{"data":[
          {"type":"sell","platinum":10,"quantity":1,"visible":true,"rank":0,"itemId":"i1","user":{"status":"ingame","ingameName":"r0"}},
          {"type":"sell","platinum":20,"quantity":1,"visible":true,"rank":5,"itemId":"i1","user":{"status":"ingame","ingameName":"r5"}},
          {"type":"buy","platinum":15,"quantity":1,"visible":true,"rank":5,"itemId":"i1","user":{"status":"online","ingameName":"b5"}},
          {"type":"buy","platinum":3,"quantity":1,"visible":true,"rank":0,"itemId":"i1","user":{"status":"online","ingameName":"b0"}}
        ]}"#;
        let filter = OrderFilter {
            rank: Some(5),
            ..OrderFilter::default()
        };
        let filtered = book_filtered(json, &filter).unwrap();
        assert_eq!(filtered.sell.len(), 1);
        assert_eq!(filtered.sell[0].ingame_name, "r5");
        assert_eq!(filtered.buy.len(), 1);

        let all = book_all(json).unwrap();
        assert_eq!(all.sell.len(), 2);
        assert_eq!(all.sell[0].ingame_name, "r0");
        assert_eq!(all.buy.len(), 2);
        assert_eq!(all.buy[0].ingame_name, "b5");
    }

    #[test]
    fn recent_keeps_only_visible_online_sells() {
        let recent = recent(BOOK).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].item_id, "i1");
        assert_eq!(recent[0].platinum, 35);
        assert_eq!(recent[1].platinum, 30);
    }

    #[test]
    fn top_requests_carry_rank_and_subtype() {
        let catalog = crate::catalog::Catalog::from_json(
            r#"{"data":[{"id":"r1","slug":"axi_a1_relic","gameRef":"/Projections/AxiA1",
               "tags":["relic"],"subtypes":["intact","radiant"],"i18n":{"en":{"name":"Axi A1 Relic"}}}]}"#,
        )
        .unwrap();
        let relic = catalog.resolve("axi_a1_relic").unwrap();
        assert_eq!(
            top_orders_url(&relic.slug, None, None),
            "https://api.warframe.market/v2/orders/item/axi_a1_relic/top"
        );
        assert_eq!(
            top_orders_url(&relic.slug, Some(3), None),
            "https://api.warframe.market/v2/orders/item/axi_a1_relic/top?rank=3"
        );
        assert_eq!(
            top_orders_url(&relic.slug, None, Some("radiant")),
            "https://api.warframe.market/v2/orders/item/axi_a1_relic/top?subtype=radiant"
        );
        assert_eq!(
            top_cache_name(relic, None, None),
            "top-axi_a1_relic-all-any.json"
        );
        assert_eq!(
            top_cache_name(relic, Some(3), Some("radiant")),
            "top-axi_a1_relic-3-radiant.json"
        );
        // A hostile subtype must never reach a path or a query string.
        assert_eq!(
            top_cache_name(relic, None, Some("../../etc")),
            "top-axi_a1_relic-all-any.json"
        );
        assert_eq!(
            top_orders_url(&relic.slug, None, Some("../../etc")),
            "https://api.warframe.market/v2/orders/item/axi_a1_relic/top"
        );
        let error =
            fetch_top_orders(relic, None, Some("radi ant"), Duration::from_secs(0)).unwrap_err();
        assert!(error.contains("invalid subtype"), "{error}");
        let error =
            fetch_top_orders(relic, None, Some("flawless"), Duration::from_secs(0)).unwrap_err();
        assert!(error.contains("unknown subtype"), "{error}");
    }

    #[test]
    fn top_orders_group_variants_like_a_book() {
        let json = r#"{"apiVersion":"0.25.0","data":{"buy":[],"sell":[
          {"type":"sell","platinum":8,"quantity":1,"visible":true,"rank":5,"user":{"status":"ingame"}},
          {"type":"sell","platinum":12,"quantity":1,"visible":true,"rank":5,"user":{"status":"online"}},
          {"type":"sell","platinum":2,"quantity":1,"visible":true,"rank":0,"user":{"status":"online"}},
          {"type":"sell","platinum":1,"quantity":1,"visible":true,"rank":0,"user":{"status":"offline"}}
        ]}}"#;
        let variants = top_variants(json).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].rank, Some(0));
        assert_eq!(variants[0].platinum, 2);
        assert_eq!(variants[1].rank, Some(5));
        assert_eq!(variants[1].platinum, 8);
        assert_eq!(variants[1].online, 2);
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;

    const VARIANTS: &str = r#"{"data":[
      {"type":"sell","platinum":50,"quantity":1,"visible":true,"rank":5,"user":{"status":"online","ingameName":"r5"}},
      {"type":"sell","platinum":10,"quantity":1,"visible":true,"rank":0,"subtype":"regular","user":{"status":"online","ingameName":"r0"}},
      {"type":"sell","platinum":30,"quantity":1,"visible":true,"amberStars":3,"cyanStars":2,"user":{"status":"online","ingameName":"filled"}}
    ]}"#;

    const CHIPS: &str = r#"{"data":[
      {"type":"sell","platinum":3,"quantity":1,"visible":true,"rank":0,"subtype":"regular","user":{"status":"online"}},
      {"type":"sell","platinum":18,"quantity":2,"visible":true,"rank":10,"subtype":"regular","user":{"status":"online"}},
      {"type":"sell","platinum":14,"quantity":3,"visible":true,"rank":10,"subtype":"regular","user":{"status":"ingame"}},
      {"type":"sell","platinum":1,"quantity":9,"visible":true,"rank":10,"subtype":"regular","user":{"status":"offline"}},
      {"type":"sell","platinum":2,"quantity":9,"visible":false,"rank":10,"subtype":"regular","user":{"status":"online"}},
      {"type":"buy","platinum":99,"quantity":9,"visible":true,"rank":10,"subtype":"regular","user":{"status":"online"}}
    ]}"#;

    #[test]
    fn groups_variants_by_rank_and_keeps_the_cheapest() {
        let variants = variants(CHIPS).unwrap();
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].rank, Some(0));
        assert_eq!(variants[0].platinum, 3);
        assert_eq!(variants[0].online, 1);
        assert_eq!(variants[1].rank, Some(10));
        assert_eq!(variants[1].platinum, 14);
        assert_eq!(variants[1].online, 2);
        assert_eq!(variants[1].quantity, 5);
    }

    #[test]
    fn default_filter_matches_the_max_variant() {
        let catalog = crate::catalog::Catalog::from_json(
            r#"{"data":[
              {"slug":"vitality","gameRef":"/Vitality","tags":["mod"],"maxRank":10,"subtypes":["regular"],
               "i18n":{"en":{"name":"Vitality"}}}
            ]}"#,
        )
        .unwrap();
        let item = catalog.resolve("vitality").unwrap();
        let filter = default_filter(item);
        assert_eq!(filter.rank, Some(10));
        assert_eq!(filter.subtype.as_deref(), Some("regular"));
        let book = book_filtered(CHIPS, &filter).unwrap();
        assert_eq!(book.min_sell, Some(14));
    }

    #[test]
    fn filters_orders_by_rank_and_stars() {
        let rank = OrderFilter {
            rank: Some(5),
            ..Default::default()
        };
        let book = book_filtered(VARIANTS, &rank).unwrap();
        assert_eq!(book.min_sell, Some(50));
        assert_eq!(book.sell[0].rank, Some(5));
        assert_eq!(book.online, 1);

        let stars = OrderFilter {
            amber_stars: Some(3),
            cyan_stars: Some(2),
            ..Default::default()
        };
        let book = book_filtered(VARIANTS, &stars).unwrap();
        assert_eq!(book.min_sell, Some(30));
        assert_eq!(book.sell[0].amber_stars, Some(3));
        assert_eq!(book.sell[0].cyan_stars, Some(2));
    }
}

#[cfg(test)]
mod recent_variant_tests {
    use super::*;

    const RECENT: &str = r#"{"data":[
      {"type":"sell","platinum":20,"quantity":1,"visible":true,"itemId":"i1","rank":10,"user":{"status":"online","ingameName":"a"}},
      {"type":"sell","platinum":18,"quantity":2,"visible":true,"itemId":"i1","rank":10,"user":{"status":"ingame","ingameName":"b"}},
      {"type":"sell","platinum":3,"quantity":4,"visible":true,"itemId":"i1","rank":0,"user":{"status":"online","ingameName":"c"}},
      {"type":"sell","platinum":50,"quantity":1,"visible":true,"itemId":"i1","rank":5,"user":{"status":"online","ingameName":"d"}}
    ]}"#;

    #[test]
    fn groups_recent_orders_by_rank() {
        let orders = recent(RECENT).unwrap();
        let variants = recent_variants(&orders);
        assert_eq!(variants.len(), 3);
        let rank_10 = variants
            .iter()
            .find(|variant| variant.rank == Some(10))
            .unwrap();
        assert_eq!(rank_10.platinum, 18);
        assert_eq!(rank_10.online, 2);
        assert_eq!(rank_10.quantity, 3);
        assert_eq!(variants[0].rank, Some(0));
    }

    #[test]
    fn default_filter_picks_the_max_recent_variant() {
        let catalog = crate::catalog::Catalog::from_json(
            r#"{"data":[{"id":"i1","slug":"vitality","gameRef":"/Vitality","tags":["mod"],
               "maxRank":10,"subtypes":["regular"],"i18n":{"en":{"name":"Vitality"}}}]}"#,
        )
        .unwrap();
        let item = catalog.resolve("vitality").unwrap();
        let orders = recent(
            r#"{"data":[
              {"type":"sell","platinum":3,"quantity":1,"visible":true,"itemId":"i1","rank":0,"subtype":"regular","user":{"status":"online","ingameName":"c"}},
              {"type":"sell","platinum":18,"quantity":1,"visible":true,"itemId":"i1","rank":10,"subtype":"regular","user":{"status":"online","ingameName":"b"}}
            ]}"#,
        )
        .unwrap();
        let filter = default_filter(item);
        let variants = recent_variants(&orders);
        let default = variants
            .iter()
            .find(|variant| filter.matches_variant(variant))
            .unwrap();
        assert_eq!(default.platinum, 18);
    }
}
