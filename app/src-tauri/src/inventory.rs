use std::collections::{HashMap, HashSet};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use coruframe_core::catalog::Catalog;
use coruframe_core::fetch;
use coruframe_core::inventory_keys;
use coruframe_core::itemdb::{is_equipment, is_kitgun_chamber, ItemDb, ItemEntry};
use coruframe_core::mastery::Mastery;

use crate::equipment::{self, ItemDto};
use crate::market::{self, MarketItemDto};

#[derive(Clone, Serialize)]
pub struct InventoryItemDto {
    pub item: ItemDto,
    pub market: Option<MarketItemDto>,
    pub count: u64,
    pub kind: String,
    pub sellable: bool,
    pub built: bool,
    pub rank: Option<u64>,
    pub tags: Vec<String>,
    pub mastered: Option<bool>,
    pub resolved: bool,
    pub subtype: Option<String>,
    pub set: Option<SetRefDto>,
}

#[derive(Clone, Serialize)]
pub struct SetRefDto {
    pub name: String,
    pub slug: String,
    pub have: usize,
    pub need: usize,
}

#[derive(Clone, Serialize)]
pub struct ArrayStatusDto {
    pub source: String,
    pub observed_at: Option<u64>,
}

#[derive(Serialize)]
pub struct InventorySnapshotDto {
    pub linked: bool,
    pub captured_at: u64,
    pub complete: bool,
    pub captured: Vec<String>,
    pub missing: Vec<String>,
    pub schema: Option<u64>,
    pub limit: Option<String>,
    pub array_status: HashMap<String, ArrayStatusDto>,
    pub total: usize,
    pub sellable: usize,
    pub unresolved: usize,
    pub mastery_available: bool,
    pub mastery: Mastery,
    pub items: Vec<InventoryItemDto>,
    pub component_counts: HashMap<String, u64>,
}

// Core arrays are required for a snapshot to replace the live file; category
// arrays are optional and only gate their own sections. Mod ranks/counts are
// required too: a snapshot without them would report every mod unranked.
const REQUIRED_ARRAYS: [&str; 6] = inventory_keys::CORE;
const OPTIONAL_ARRAYS: [&str; 8] = inventory_keys::CATEGORY;

struct Row {
    path: String,
    kind: &'static str,
    count: u64,
    rank: Option<u64>,
    // Built kitguns are generic entries whose ModularParts name the chamber.
    chamber: Option<String>,
}

fn array_kind(key: &str) -> Option<&'static str> {
    Some(match key {
        "MiscItems" => "misc",
        "Suits" => "warframe",
        "Sentinels" => "companion",
        "KubrowPets" => "companion",
        "MechSuits" => "necramech",
        "LongGuns" => "primary",
        "Pistols" => "secondary",
        "Melee" => "melee",
        "SentinelWeapons" => "companion_weapon",
        "OperatorAmps" => "amp",
        "SpaceSuits" => "archwing",
        "SpaceGuns" => "archgun",
        "SpaceMelee" => "archmelee",
        "RawUpgrades" => "mod",
        "Upgrades" => "upgrade",
        // XPInfo is mastery history, not an owned-item list; the row loop skips
        // it because its entries repeat every mastered ItemType.
        "XPInfo" => "mastery",
        _ => return None,
    })
}

fn derive_status(value: &serde_json::Value) -> (bool, Vec<String>, Vec<String>) {
    let present = |key: &str| value.get(key).is_some_and(serde_json::Value::is_array);
    let gates = || {
        REQUIRED_ARRAYS
            .iter()
            .chain(OPTIONAL_ARRAYS.iter())
            .chain(inventory_keys::RANKS.iter())
    };
    let captured: Vec<String> = gates()
        .filter(|key| present(key))
        .map(|key| key.to_string())
        .collect();
    let missing: Vec<String> = gates()
        .filter(|key| !present(key))
        .map(|key| key.to_string())
        .collect();
    (gates().all(|key| present(key)), captured, missing)
}

// The live snapshot is the last complete one. `complete` and `missing` describe
// what was actually loaded, so a later partial scan cannot downgrade a good
// snapshot to "zero owned"; the scan report stays a scanner diagnostic.
// Derive from the arrays, not the scanner's flag: an older snapshot may carry a
// stale `_meta.complete` that contradicts what was actually written.
fn snapshot_meta(value: &serde_json::Value) -> (bool, Vec<String>, Vec<String>) {
    derive_status(value)
}

// Per-array provenance is scanner metadata; a legacy snapshot without it
// yields an empty map and renders exactly as before.
fn array_status(value: &serde_json::Value) -> HashMap<String, ArrayStatusDto> {
    value
        .get("_meta")
        .and_then(|meta| meta.get("arrays"))
        .and_then(serde_json::Value::as_object)
        .map(|arrays| {
            arrays
                .iter()
                .filter_map(|(key, entry)| {
                    let source = entry.get("source").and_then(serde_json::Value::as_str)?;
                    Some((
                        key.clone(),
                        ArrayStatusDto {
                            source: source.to_string(),
                            observed_at: entry
                                .get("observed_at")
                                .and_then(serde_json::Value::as_u64),
                        },
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

// A built kitgun is a generic entry; its chamber is either the ItemType itself
// or one of the ModularParts. Other modular gear (zaws, amps) resolves to a
// non-chamber entry and is ignored here.
fn kitgun_chamber(db: &ItemDb, entry: &serde_json::Value) -> Option<String> {
    if let Some(path) = entry.get("ItemType").and_then(serde_json::Value::as_str) {
        if db.resolve(path).is_some_and(is_kitgun_chamber) {
            return Some(path.to_string());
        }
    }
    entry
        .get("ModularParts")
        .and_then(serde_json::Value::as_array)?
        .iter()
        .filter_map(serde_json::Value::as_str)
        .find(|part| db.resolve(part).is_some_and(is_kitgun_chamber))
        .map(str::to_string)
}

fn built_kind(kind: &str) -> bool {
    matches!(
        kind,
        "warframe"
            | "primary"
            | "secondary"
            | "melee"
            | "companion"
            | "archwing"
            | "archgun"
            | "archmelee"
            | "necramech"
    )
}

// The wfm catalog only contains tradable listings, so a market join is the
// real signal. Only pseudo-items that cannot be traded from inventory are out.
fn sellable(tags: &[String]) -> bool {
    const PSEUDO: [&str; 7] = [
        "set",
        "scene",
        "simulacrum",
        "skin",
        "glyph",
        "avatar",
        "ship",
    ];
    !tags.iter().any(|tag| PSEUDO.contains(&tag.as_str()))
}

fn fingerprint_level(entry: &serde_json::Value) -> Option<u64> {
    let value = entry.get("UpgradeFingerprint")?;
    let parsed: serde_json::Value = match value {
        serde_json::Value::String(text) => serde_json::from_str(text).ok()?,
        other => other.clone(),
    };
    parsed.get("lvl").and_then(serde_json::Value::as_u64)
}

fn entry_rank(entry: &serde_json::Value, kind: &str) -> Option<u64> {
    if kind != "mod" && kind != "upgrade" {
        return None;
    }
    // A mod/arcane entry without a fingerprint is an unranked copy.
    Some(fingerprint_level(entry).unwrap_or(0))
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn unknown_item(path: &str, kind: &str) -> ItemDto {
    ItemDto {
        unique_name: path.to_string(),
        name: basename(path).to_string(),
        category: String::new(),
        kind: kind.to_string(),
        item_type: None,
        image_url: None,
        tradable: false,
        masterable: false,
        mastery_req: None,
        vaulted: false,
        prime: false,
        build_price: None,
        build_time: None,
        build_quantity: None,
        skip_build_time_price: None,
        bp_cost: None,
        mastery_cap: None,
    }
}

struct PrimeSet {
    name: String,
    market: MarketItemDto,
    parts: Vec<String>,
}

fn prime_sets(db: &ItemDb, catalog: &Catalog) -> Vec<PrimeSet> {
    let mut sets = Vec::new();
    for set in catalog.all() {
        if !set.tags.iter().any(|tag| tag == "set") || !set.tags.iter().any(|tag| tag == "prime") {
            continue;
        }
        let name = set.name().strip_suffix(" Set").unwrap_or(set.name());
        let equipment = db
            .resolve(&set.game_ref)
            .or_else(|| db.resolve(name))
            .filter(|entry| is_equipment(&entry.kind));
        let Some(equipment) = equipment else {
            continue;
        };
        let parts: Vec<String> = equipment::components(db, catalog, equipment)
            .into_iter()
            .map(|component| component.unique_name)
            .collect();
        if parts.len() < 2 {
            continue;
        }
        sets.push(PrimeSet {
            name: name.to_string(),
            market: market::item_dto(set),
            parts,
        });
    }
    sets
}

fn row_item(entry: Option<&ItemEntry>, path: &str, kind: &str) -> ItemDto {
    entry.map_or_else(|| unknown_item(path, kind), equipment::item_dto)
}

fn empty_snapshot(linked: bool) -> InventorySnapshotDto {
    InventorySnapshotDto {
        linked,
        captured_at: 0,
        complete: false,
        captured: Vec::new(),
        missing: REQUIRED_ARRAYS
            .iter()
            .chain(OPTIONAL_ARRAYS.iter())
            .chain(inventory_keys::RANKS.iter())
            .map(|key| key.to_string())
            .collect(),
        schema: None,
        limit: None,
        array_status: HashMap::new(),
        total: 0,
        sellable: 0,
        unresolved: 0,
        mastery_available: equipment::mastery_available(),
        mastery: Mastery::default(),
        items: Vec::new(),
        component_counts: HashMap::new(),
    }
}

// Riven mods keep their stats in the fingerprint, which the inventory DTO does
// not carry. The tracker reads the raw entries instead of widening the
// snapshot that every view pays for.
pub fn raw_upgrades() -> Result<Vec<serde_json::Value>, String> {
    let path = fetch::cache_dir().join("inventory-raw.json");
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Ok(Vec::new()),
    };
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| format!("inventory json: {error}"))?;
    Ok(value
        .get("Upgrades")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default())
}

pub fn load(db: &ItemDb, catalog: &Catalog) -> Result<InventorySnapshotDto, String> {
    let path = fetch::cache_dir().join("inventory-raw.json");
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(_) => return Ok(empty_snapshot(false)),
    };
    let captured_at = fs::metadata(&path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|now| now.as_secs().saturating_sub(age.as_secs()))
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|_| "snapshot unreadable".to_string())?;
    load_value(db, catalog, value, captured_at)
}

fn load_value(
    db: &ItemDb,
    catalog: &Catalog,
    value: serde_json::Value,
    captured_at: u64,
) -> Result<InventorySnapshotDto, String> {
    let mut rows: HashMap<(String, Option<u64>, &'static str), Row> = HashMap::new();
    for key in inventory_keys::TRACKED {
        let Some(kind) = array_kind(key) else {
            continue;
        };
        if kind == "mastery" {
            continue;
        }
        let Some(entries) = value.get(key).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for entry in entries {
            // Upgrades is a list of equipped ObjectIds, not inventory entries.
            let Some(item_path) = entry.get("ItemType").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let count = entry
                .get("ItemCount")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1);
            let rank = entry_rank(entry, kind);
            let chamber = matches!(kind, "primary" | "secondary")
                .then(|| kitgun_chamber(db, entry))
                .flatten();
            rows.entry((item_path.to_string(), rank, kind))
                .and_modify(|row| row.count += count)
                .or_insert(Row {
                    path: item_path.to_string(),
                    kind,
                    count,
                    rank,
                    chamber,
                });
        }
    }

    let mastery_available = value.get("XPInfo").is_some_and(serde_json::Value::is_array);
    let mastery = Mastery::from_profile(&value, db);
    let mastered: HashSet<&str> = mastery.mastered.iter().map(String::as_str).collect();
    let mut component_counts: HashMap<String, u64> = HashMap::new();
    for row in rows.values() {
        *component_counts.entry(row.path.clone()).or_default() += row.count;
    }

    let prime = prime_sets(db, catalog);
    // A component can appear in more than one set in theory; the first one wins
    // because the line is informational, not a pricing authority.
    let mut part_sets: HashMap<String, usize> = HashMap::new();
    for (index, set) in prime.iter().enumerate() {
        for part in &set.parts {
            part_sets.entry(part.clone()).or_insert(index);
        }
    }

    let mut items = Vec::with_capacity(rows.len());
    for ((path, _, _), row) in &rows {
        let chamber = row.chamber.as_deref().and_then(|path| db.resolve(path));
        let entry = chamber.or_else(|| db.resolve(path));
        let market = entry.and_then(|entry| equipment::market_item(catalog, entry));
        let built = built_kind(row.kind);
        let tags = market
            .as_ref()
            .map(|item| item.tags.clone())
            .unwrap_or_default();
        let sellable = market.is_some() && !built && sellable(&tags);
        let set = entry
            .and_then(|entry| part_sets.get(&entry.unique_name))
            .map(|index| {
                let set = &prime[*index];
                let have = set
                    .parts
                    .iter()
                    .filter(|part| component_counts.get(*part).copied().unwrap_or(0) > 0)
                    .count();
                SetRefDto {
                    name: set.name.clone(),
                    slug: set.market.slug.clone(),
                    have,
                    need: set.parts.len(),
                }
            });
        let mut item = row_item(entry, path, row.kind);
        if let Some(chamber) = chamber {
            // The collection card is the chamber, not the built weapon's
            // custom name; the array tells us which synthetic slot it is.
            item.unique_name = format!("{}#{}", chamber.unique_name, row.kind);
            item.item_type = Some("Kitgun".to_string());
            item.kind = row.kind.to_string();
            item.masterable = true;
            item.mastery_cap = Some(coruframe_core::mastery::max_xp(chamber));
        }
        let mastered = mastery_available.then(|| mastered.contains(item.unique_name.as_str()));
        items.push(InventoryItemDto {
            item,
            market,
            count: row.count,
            kind: if chamber.is_some() {
                row.kind.to_string()
            } else {
                entry
                    .map_or(row.kind, |entry| entry.kind.as_str())
                    .to_string()
            },
            sellable,
            built,
            rank: row.rank,
            tags,
            mastered,
            resolved: entry.is_some(),
            subtype: entry.and_then(equipment::relic_subtype),
            set,
        });
    }
    items.sort_by(|left, right| {
        left.item
            .name
            .cmp(&right.item.name)
            .then(left.rank.cmp(&right.rank))
    });
    let sellable = items.iter().filter(|item| item.sellable).count();
    let unresolved = items.iter().filter(|item| !item.resolved).count();

    let (complete, captured, missing) = snapshot_meta(&value);
    let meta = value.get("_meta");
    Ok(InventorySnapshotDto {
        linked: true,
        captured_at,
        complete,
        captured,
        missing,
        schema: meta
            .and_then(|meta| meta.get("schema"))
            .and_then(serde_json::Value::as_u64),
        limit: meta
            .and_then(|meta| meta.get("limit"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
        array_status: array_status(&value),
        total: items.len(),
        sellable,
        unresolved,
        mastery_available,
        mastery,
        component_counts,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_rank_from_mod_and_upgrade_entries() {
        let fingerprint = json!({ "lvl": 5 }).to_string();
        assert_eq!(
            entry_rank(&json!({ "UpgradeFingerprint": fingerprint }), "mod"),
            Some(5)
        );
        assert_eq!(
            entry_rank(&json!({ "UpgradeFingerprint": { "lvl": 3 } }), "upgrade"),
            Some(3)
        );
        assert_eq!(entry_rank(&json!({}), "mod"), Some(0));
        assert_eq!(
            entry_rank(&json!({ "UpgradeFingerprint": "{\"lvl\":2}" }), "warframe"),
            None
        );
    }

    const SET_EQUIPMENT: &str = r#"[
      {"uniqueName":"/Lotus/Types/Sentinels/SentinelPowersuits/PrimeCarrierPowerSuit",
       "name":"Carrier Prime","category":"Sentinels","type":"Sentinel",
       "masterable":true,"isPrime":true,
       "components":[
         {"uniqueName":"/P/Blueprint","itemCount":1},
         {"uniqueName":"/P/Carapace","itemCount":1}
       ]}
    ]"#;

    const SET_PARTS: &str = r#"[
      {"uniqueName":"/P/Blueprint","name":"Carrier Prime Blueprint","category":"Components",
       "parentUniqueNames":["/Lotus/Types/Sentinels/SentinelPowersuits/PrimeCarrierPowerSuit"]},
      {"uniqueName":"/P/Carapace","name":"Carrier Prime Carapace","category":"Components",
       "parentUniqueNames":["/Lotus/Types/Sentinels/SentinelPowersuits/PrimeCarrierPowerSuit"]}
    ]"#;

    const SET_MARKET: &str = r#"{"data":[
      {"id":"s1","slug":"carrier_prime_set",
       "gameRef":"/Lotus/Types/Sentinels/SentinelPowersuits/PrimeCarrierPowerSuit",
       "tags":["set","prime"],"i18n":{"en":{"name":"Carrier Prime Set"}}}
    ]}"#;

    #[test]
    fn maps_components_to_their_prime_set() {
        let db = ItemDb::from_categories(
            &[("Sentinels", SET_EQUIPMENT), ("Components", SET_PARTS)],
            0,
        )
        .unwrap();
        let catalog = Catalog::from_json(SET_MARKET).unwrap();
        let sets = prime_sets(&db, &catalog);
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].name, "Carrier Prime");
        assert_eq!(sets[0].market.slug, "carrier_prime_set");
        assert_eq!(sets[0].parts, ["/P/Blueprint", "/P/Carapace"]);
    }

    const RELICS: &str = r#"[
      {"uniqueName":"/Lotus/Types/Game/Projections/T4VoidProjectionEBronze","name":"Axi A1 Intact","category":"Relics","type":"Relic","tradable":true},
      {"uniqueName":"/Lotus/Types/Game/Projections/T4VoidProjectionEPlatinum","name":"Axi A1 Radiant","category":"Relics","type":"Relic","tradable":true},
      {"uniqueName":"/Lotus/Types/Game/Projections/T5VoidProjectionImmortalOmniA","name":"Requiem Eterna Relic","category":"Relics","type":"Relic","tradable":true}
    ]"#;

    const RELIC_MARKET: &str = r#"{"data":[
      {"id":"r1","slug":"axi_a1_relic","gameRef":"/Lotus/Types/Game/Projections/T4VoidProjectionE",
       "tags":["relic","axi"],"subtypes":["intact","exceptional","flawless","radiant"],
       "i18n":{"en":{"name":"Axi A1 Relic"}}},
      {"id":"r2","slug":"requiem_eterna_relic","gameRef":"/Lotus/Types/Game/Projections/T5VoidProjectionImmortalOmniA",
       "tags":["relic","requiem"],"subtypes":["intact","exceptional","flawless","radiant"],
       "i18n":{"en":{"name":"Requiem Eterna Relic"}}}
    ]}"#;

    #[test]
    fn relic_refinements_share_the_market_family() {
        let db = ItemDb::from_categories(&[("Relics", RELICS)], 0).unwrap();
        let catalog = Catalog::from_json(RELIC_MARKET).unwrap();
        for path in [
            "/Lotus/Types/Game/Projections/T4VoidProjectionEBronze",
            "/Lotus/Types/Game/Projections/T4VoidProjectionESilver",
            "/Lotus/Types/Game/Projections/T4VoidProjectionEGold",
            "/Lotus/Types/Game/Projections/T4VoidProjectionEPlatinum",
        ] {
            let base = equipment::relic_base(path);
            assert_eq!(
                catalog.resolve(base).map(|item| item.slug.as_str()),
                Some("axi_a1_relic"),
                "{path}"
            );
        }
        let requiem = db
            .resolve("/Lotus/Types/Game/Projections/T5VoidProjectionImmortalOmniA")
            .unwrap();
        assert_eq!(equipment::relic_subtype(requiem), None);
        assert_eq!(
            equipment::market_item(&catalog, requiem).map(|item| item.slug),
            Some("requiem_eterna_relic".to_string())
        );
    }

    #[test]
    fn marks_a_legacy_partial_snapshot_as_incomplete() {
        let db = ItemDb::from_categories(&[], 0).unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let snapshot = load_value(&db, &catalog, json!({"MiscItems": [], "Suits": []}), 0).unwrap();
        assert!(!snapshot.complete);
        assert!(snapshot.missing.contains(&"LongGuns".to_string()));
        assert!(snapshot.missing.contains(&"XPInfo".to_string()));
        assert!(!snapshot.mastery_available);
    }

    #[test]
    fn a_complete_flag_cannot_override_missing_arrays() {
        let db = ItemDb::from_categories(&[], 0).unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let snapshot = load_value(
            &db,
            &catalog,
            json!({
                "MiscItems": [],
                "_meta": {
                    "complete": true,
                    "captured": ["MiscItems", "Suits"],
                    "missing": []
                }
            }),
            0,
        )
        .unwrap();
        assert!(!snapshot.complete);
        assert!(snapshot.missing.contains(&"LongGuns".to_string()));
        assert!(snapshot.captured.contains(&"MiscItems".to_string()));
    }

    #[test]
    fn derives_complete_from_all_tracked_arrays() {
        let db = ItemDb::from_categories(&[], 0).unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let snapshot = load_value(
            &db,
            &catalog,
            json!({
                "MiscItems": [], "Suits": [], "LongGuns": [], "Pistols": [],
                "Melee": [], "XPInfo": [], "Sentinels": [], "SentinelWeapons": [],
                "KubrowPets": [], "MechSuits": [], "SpaceSuits": [],
                "SpaceGuns": [], "SpaceMelee": [], "OperatorAmps": [],
                "RawUpgrades": [], "Upgrades": []
            }),
            0,
        )
        .unwrap();
        assert!(snapshot.complete);
        assert!(snapshot.missing.is_empty());
        assert!(snapshot.captured.contains(&"MechSuits".to_string()));
    }

    #[test]
    fn ranks_and_counts_require_both_upgrade_arrays() {
        let db = ItemDb::from_categories(&[], 0).unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let mut arrays = json!({
            "MiscItems": [], "Suits": [], "LongGuns": [], "Pistols": [],
            "Melee": [], "XPInfo": [], "Sentinels": [], "SentinelWeapons": [],
            "KubrowPets": [], "MechSuits": [], "SpaceSuits": [],
            "SpaceGuns": [], "SpaceMelee": [], "OperatorAmps": [],
            "RawUpgrades": []
        });
        let partial = load_value(&db, &catalog, arrays.clone(), 0).unwrap();
        assert!(!partial.complete);
        assert!(partial.missing.contains(&"Upgrades".to_string()));

        arrays["Upgrades"] = json!([]);
        let complete = load_value(&db, &catalog, arrays, 0).unwrap();
        assert!(complete.complete);
        assert!(complete.missing.is_empty());
    }

    #[test]
    fn reads_sentinel_and_sentinel_weapon_arrays() {
        let db = ItemDb::from_categories(
            &[
                (
                    "Primary",
                    r#"[{"uniqueName":"/Lotus/Types/Sentinels/SentinelWeapons/Gremlin",
                         "name":"Artax","category":"Primary","type":"Companion Weapon",
                         "masterable":true}]"#,
                ),
                (
                    "Sentinels",
                    r#"[{"uniqueName":"/Lotus/Types/Sentinels/TennoSentinel",
                         "name":"Wyrm","category":"Sentinels","masterable":true}]"#,
                ),
                (
                    "Pets",
                    r#"[{"uniqueName":"/Lotus/Types/Game/KubrowPet/KubrowPetPowerSuit",
                         "name":"Sunika Kubrow","category":"Pets","type":"Pets",
                         "masterable":true}]"#,
                ),
                (
                    "Warframes",
                    r#"[{"uniqueName":"/Lotus/Powersuits/EntratiMech/NechroTech",
                         "name":"Voidrig","category":"Warframes","type":"Necramech",
                         "masterable":true}]"#,
                ),
                (
                    "Misc",
                    r#"[{"uniqueName":"/Lotus/AmpPrism","name":"Cantic Prism",
                         "category":"Misc","type":"Amp"},
                        {"uniqueName":"/Lotus/Kitgun/Vermisplicer","name":"Vermisplicer",
                         "category":"Misc","type":"Pistol"}]"#,
                ),
            ],
            0,
        )
        .unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let raw = json!({
            "SentinelWeapons": [
                {"ItemType": "/Lotus/Types/Sentinels/SentinelWeapons/Gremlin", "ItemCount": 1}
            ],
            "Sentinels": [
                {"ItemType": "/Lotus/Types/Sentinels/TennoSentinel", "ItemCount": 1}
            ],
            "KubrowPets": [
                {"ItemType": "/Lotus/Types/Game/KubrowPet/KubrowPetPowerSuit", "ItemCount": 1}
            ],
            "MechSuits": [
                {"ItemType": "/Lotus/Powersuits/EntratiMech/NechroTech", "ItemCount": 1}
            ],
            "OperatorAmps": [
                {"ItemType": "/Lotus/AmpPrism", "ItemCount": 1}
            ],
            "LongGuns": [
                {"ItemType": "/Lotus/Kitgun/Vermisplicer", "ItemCount": 1}
            ],
            "Pistols": [
                {"ItemType": "/Lotus/Kitgun/Vermisplicer", "ItemCount": 1}
            ],
        });
        let snapshot = load_value(&db, &catalog, raw, 0).unwrap();
        let artax = snapshot
            .items
            .iter()
            .find(|item| item.item.name == "Artax")
            .unwrap();
        assert!(artax.resolved);
        assert_eq!(artax.kind, "companion_weapon");
        let wyrm = snapshot
            .items
            .iter()
            .find(|item| item.item.name == "Wyrm")
            .unwrap();
        assert!(wyrm.resolved);
        assert_eq!(wyrm.kind, "companion");
        let amp = snapshot
            .items
            .iter()
            .find(|item| item.item.name == "Cantic Prism")
            .unwrap();
        assert_eq!(amp.kind, "amp");
        let kitguns: Vec<&str> = snapshot
            .items
            .iter()
            .filter(|item| item.item.name == "Vermisplicer")
            .map(|item| item.item.unique_name.as_str())
            .collect();
        assert!(kitguns.contains(&"/Lotus/Kitgun/Vermisplicer#primary"));
        assert!(kitguns.contains(&"/Lotus/Kitgun/Vermisplicer#secondary"));
        let kubrow = snapshot
            .items
            .iter()
            .find(|item| item.item.name == "Sunika Kubrow")
            .unwrap();
        assert_eq!(kubrow.kind, "companion");
        let voidrig = snapshot
            .items
            .iter()
            .find(|item| item.item.name == "Voidrig")
            .unwrap();
        assert_eq!(voidrig.kind, "necramech");
    }

    #[test]
    fn attributes_a_built_kitgun_to_its_chamber_and_slot() {
        let db = ItemDb::from_categories(
            &[(
                "Misc",
                r#"[{"uniqueName":"/Lotus/Kitgun/Vermisplicer","name":"Vermisplicer",
                     "category":"Misc","type":"Pistol"}]"#,
            )],
            0,
        )
        .unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let raw = json!({
            "LongGuns": [{
                "ItemType": "/Lotus/Weapons/SolarisUnited/Primary/LotusModularPrimaryShotgun",
                "ModularParts": [
                    "/Lotus/Kitgun/Vermisplicer",
                    "/Lotus/Weapons/SolarisUnited/Primary/SUModularPrimarySet1/Handles/SUModularPrimaryHandleDPart"
                ],
                "ItemCount": 1
            }],
            "XPInfo": [{"ItemType": "/Lotus/Kitgun/Vermisplicer", "XP": 450_000}]
        });
        let snapshot = load_value(&db, &catalog, raw, 0).unwrap();
        let kitgun = snapshot
            .items
            .iter()
            .find(|item| item.item.name == "Vermisplicer")
            .unwrap();
        assert!(kitgun.resolved);
        assert_eq!(kitgun.kind, "primary");
        assert_eq!(
            kitgun.item.unique_name,
            "/Lotus/Kitgun/Vermisplicer#primary"
        );
        assert_eq!(kitgun.item.item_type.as_deref(), Some("Kitgun"));
        assert_eq!(kitgun.item.kind, "primary");
        assert!(kitgun.item.masterable);
        assert_eq!(kitgun.item.mastery_cap, Some(450_000));
        assert_eq!(kitgun.mastered, Some(true));
    }

    #[test]
    fn inventory_keeps_one_row_per_owned_refinement() {
        let db = ItemDb::from_categories(&[("Relics", RELICS)], 0).unwrap();
        let catalog = Catalog::from_json(RELIC_MARKET).unwrap();
        let raw = json!({
            "MiscItems": [
                {"ItemType": "/Lotus/Types/Game/Projections/T4VoidProjectionEBronze", "ItemCount": 3},
                {"ItemType": "/Lotus/Types/Game/Projections/T4VoidProjectionEPlatinum", "ItemCount": 1},
                {"ItemType": "/Lotus/Types/Game/Projections/T5VoidProjectionImmortalOmniA", "ItemCount": 2}
            ]
        });
        let snapshot = load_value(&db, &catalog, raw, 0).unwrap();
        assert_eq!(snapshot.items.len(), 3);
        assert_eq!(
            snapshot
                .items
                .iter()
                .filter(|item| item.kind == "misc")
                .count(),
            0
        );
        let axi: Vec<&InventoryItemDto> = snapshot
            .items
            .iter()
            .filter(|item| {
                item.market
                    .as_ref()
                    .is_some_and(|market| market.slug == "axi_a1_relic")
            })
            .collect();
        assert_eq!(axi.len(), 2);
        let mut subtypes: Vec<&str> = axi
            .iter()
            .filter_map(|item| item.subtype.as_deref())
            .collect();
        subtypes.sort();
        assert_eq!(subtypes, ["intact", "radiant"]);
        let requiem = snapshot
            .items
            .iter()
            .find(|item| item.item.name == "Requiem Eterna Relic")
            .unwrap();
        assert_eq!(requiem.subtype, None);
    }

    #[test]
    fn market_tags_only_exclude_pseudo_items() {
        let tags = |values: &[&str]| {
            values
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        };
        for allowed in [
            &["prime", "part"][..],
            &["weapon", "part"][..],
            &["arcane_enhancement"][..],
            &["ayatan_star"][..],
        ] {
            assert!(sellable(&tags(allowed)), "{allowed:?}");
        }
        for blocked in [&["set"][..], &["scene"][..], &["skin"][..]] {
            assert!(!sellable(&tags(blocked)), "{blocked:?}");
        }
    }

    #[test]
    fn every_tracked_key_has_an_array_kind() {
        for key in inventory_keys::TRACKED {
            assert!(array_kind(key).is_some(), "{key}");
        }
        assert_eq!(array_kind("Recipes"), None);
    }

    #[test]
    fn parses_carried_and_synthesized_array_status() {
        let db = ItemDb::from_categories(&[], 0).unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let snapshot = load_value(
            &db,
            &catalog,
            json!({
                "_meta": {
                    "schema": 2,
                    "limit": "cluster_incomplete",
                    "arrays": {
                        "KubrowPets": {"source": "carried", "count": 1, "observed_at": 100},
                        "MechSuits": {"source": "synthesized", "count": 0, "observed_at": 200}
                    }
                }
            }),
            0,
        )
        .unwrap();
        assert_eq!(snapshot.schema, Some(2));
        assert_eq!(snapshot.limit.as_deref(), Some("cluster_incomplete"));
        assert_eq!(snapshot.array_status["KubrowPets"].source, "carried");
        assert_eq!(snapshot.array_status["KubrowPets"].observed_at, Some(100));
        assert_eq!(snapshot.array_status["MechSuits"].source, "synthesized");
        assert_eq!(snapshot.array_status["MechSuits"].observed_at, Some(200));
    }

    #[test]
    fn legacy_snapshots_have_no_array_status() {
        let db = ItemDb::from_categories(&[], 0).unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let snapshot = load_value(&db, &catalog, json!({"MiscItems": []}), 0).unwrap();
        assert!(snapshot.array_status.is_empty());
        assert_eq!(snapshot.schema, None);
        assert_eq!(snapshot.limit, None);
    }
}
