use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use serde::Serialize;

use coruframe_core::catalog::{Catalog, Item};
use coruframe_core::fetch;
use coruframe_core::itemdb::{is_equipment, ComponentRef, ItemDb, ItemEntry};
use coruframe_core::mastery;

use crate::market::{self, MarketItemDto};

pub type ItemDbCache = Mutex<Option<Arc<ItemDb>>>;

#[derive(Clone, Serialize)]
pub struct ItemDto {
    pub unique_name: String,
    pub name: String,
    pub category: String,
    pub kind: String,
    pub item_type: Option<String>,
    pub image_url: Option<String>,
    pub tradable: bool,
    pub masterable: bool,
    pub mastery_req: Option<u64>,
    pub vaulted: bool,
    pub prime: bool,
    pub build_price: Option<u64>,
    pub build_time: Option<u64>,
    pub build_quantity: Option<u64>,
    pub skip_build_time_price: Option<u64>,
    pub bp_cost: Option<u64>,
    pub mastery_cap: Option<u64>,
}

#[derive(Clone, Serialize)]
pub struct ComponentDto {
    pub unique_name: String,
    pub name: String,
    pub count: u64,
    pub tradable: bool,
    pub market_slug: Option<String>,
    // Relative market asset path for the part's own art; the frontend builds
    // the host URL and falls back to a generic icon when this is absent.
    pub image: Option<String>,
}

#[derive(Serialize)]
pub struct CatalogCardDto {
    pub item: ItemDto,
    pub market: Option<MarketItemDto>,
    pub components: Vec<ComponentDto>,
}

#[derive(Serialize)]
pub struct ItemDbStatusDto {
    pub available: bool,
    pub path: String,
    pub items: usize,
    pub age_secs: Option<u64>,
    pub fetched_at: u64,
    pub source: String,
    pub categories: BTreeMap<String, usize>,
    pub mastery_available: bool,
    pub error: Option<String>,
}

pub fn load(cache: &ItemDbCache) -> Result<Arc<ItemDb>, String> {
    let mut slot = cache
        .lock()
        .map_err(|_| "itemdb cache poisoned".to_string())?;
    if let Some(db) = slot.as_ref() {
        return Ok(db.clone());
    }
    let db = Arc::new(ItemDb::load()?);
    *slot = Some(db.clone());
    Ok(db)
}

// Rebuild the database from the network and swap it in. A failed build leaves
// the working database untouched; a second caller while one runs gets the
// current status instead of starting a parallel build.
pub fn update(
    cache: &ItemDbCache,
    updating: &AtomicBool,
    force: bool,
) -> Result<ItemDbStatusDto, String> {
    if updating.swap(true, Ordering::AcqRel) {
        let db = cache.lock().ok().and_then(|slot| slot.clone());
        return Ok(status(db.as_deref(), None));
    }
    let built = ItemDb::build(force);
    updating.store(false, Ordering::Release);
    match built {
        Ok(db) => {
            let db = Arc::new(db);
            if let Ok(mut slot) = cache.lock() {
                *slot = Some(db.clone());
            }
            Ok(status(Some(&db), None))
        }
        Err(error) => {
            eprintln!("itemdb update: {error}");
            let db = cache.lock().ok().and_then(|slot| slot.clone());
            Ok(status(db.as_deref(), Some(error)))
        }
    }
}

// The item DB is remote data; its image name is one path component on a fixed
// host, never a URL of its own.
pub fn image_url(image: Option<&str>) -> Option<String> {
    let name = image?;
    // No separators, scheme or query bytes: the name is one path segment on a
    // fixed host. Apostrophes are legal in the WFCD filenames.
    let valid = !name.is_empty()
        && name.len() <= 200
        && !name.starts_with('.')
        && !name.contains("..")
        && !name.bytes().any(|byte| {
            byte.is_ascii_control()
                || matches!(
                    byte,
                    b'/' | b'\\' | b'?' | b'#' | b'%' | b':' | b'"' | b'<' | b'>' | b' '
                )
        });
    valid.then(|| format!("https://cdn.warframestat.us/img/{name}"))
}

pub fn item_dto(entry: &ItemEntry) -> ItemDto {
    ItemDto {
        unique_name: entry.unique_name.clone(),
        name: entry.name.clone(),
        category: entry.category.clone(),
        kind: entry.kind.clone(),
        item_type: entry.item_type.clone(),
        image_url: image_url(entry.image.as_deref()),
        tradable: entry.tradable,
        masterable: entry.masterable,
        mastery_req: entry.mastery_req,
        vaulted: entry.vaulted,
        prime: entry.prime,
        build_price: entry.build_price,
        build_time: entry.build_time,
        build_quantity: entry.build_quantity,
        skip_build_time_price: entry.skip_build_time_price,
        bp_cost: entry.bp_cost,
        mastery_cap: (entry.masterable && is_equipment(&entry.kind))
            .then(|| mastery::max_xp(entry)),
    }
}

// Relics are one market item with a subtype per refinement; WFCD instead lists
// each refinement as its own entry, so relic cards are built from the market
// catalog and only need the fields the UI reads.
fn market_entry(item: &Item) -> ItemDto {
    ItemDto {
        unique_name: format!("market:{}", item.slug),
        name: item.name().to_string(),
        category: "Relics".to_string(),
        kind: "relic".to_string(),
        item_type: None,
        image_url: None,
        tradable: item.tradable.unwrap_or(true),
        masterable: false,
        mastery_req: None,
        vaulted: item.vaulted.unwrap_or(false),
        prime: false,
        build_price: None,
        build_time: None,
        build_quantity: None,
        skip_build_time_price: None,
        bp_cost: None,
        mastery_cap: None,
    }
}

fn is_ayatan(tags: &[String]) -> bool {
    tags.iter()
        .any(|tag| tag == "ayatan_sculpture" || tag == "ayatan_star")
}

pub fn part_ref<'a>(catalog: &Catalog, component: &'a ComponentRef) -> Option<&'a ComponentRef> {
    // Necramech parts are tagged Resources in WFCD but are tradable on the
    // market, so component category alone is too narrow.
    (component.part || catalog.resolve(&component.unique_name).is_some()).then_some(component)
}

fn component_market<'a>(catalog: &'a Catalog, unique_name: &str) -> Option<&'a Item> {
    // WFCD lists the built component, while the market lists its blueprint.
    let blueprint = unique_name
        .strip_suffix("Component")
        .map(|prefix| format!("{prefix}Blueprint"));
    catalog
        .resolve(unique_name)
        .or_else(|| blueprint.as_deref().and_then(|path| catalog.resolve(path)))
}

pub fn components(db: &ItemDb, catalog: &Catalog, entry: &ItemEntry) -> Vec<ComponentDto> {
    let mut seen = std::collections::HashSet::new();
    entry
        .components
        .iter()
        .filter_map(|component| part_ref(catalog, component))
        // WFCD occasionally lists a stranger's part under a frame with the same
        // display name (Chroma carries a Volt helmet); the parts list is by name.
        .filter(|component| seen.insert(component.name.as_str()))
        .map(|component| {
            let market = component_market(catalog, &component.unique_name);
            ComponentDto {
                unique_name: component.unique_name.clone(),
                name: component.name.clone(),
                count: component.count,
                tradable: component.tradable,
                market_slug: market.map(|item| item.slug.clone()),
                // The market reuses the frame's render for every part; the item
                // DB carries the actual in-game part art (blueprint/chassis/...).
                image: db
                    .resolve(&component.unique_name)
                    .and_then(|part| part.image.clone()),
            }
        })
        .collect()
}

// WFCD lists every refinement of a relic as its own entry; the market lists one
// family with `subtypes`, and the refinement is what prices an order.
const REFINEMENTS: [(&str, &str); 4] = [
    ("Bronze", "intact"),
    ("Silver", "exceptional"),
    ("Gold", "flawless"),
    ("Platinum", "radiant"),
];

pub fn relic_base(unique_name: &str) -> &str {
    REFINEMENTS
        .iter()
        .find_map(|(suffix, _)| unique_name.strip_suffix(suffix))
        .unwrap_or(unique_name)
}

pub fn relic_subtype(entry: &ItemEntry) -> Option<String> {
    if entry.kind != "relic" {
        return None;
    }
    let from_path = REFINEMENTS
        .iter()
        .find(|(suffix, _)| entry.unique_name.ends_with(suffix));
    let name = entry.name.to_lowercase();
    from_path
        .or_else(|| {
            REFINEMENTS
                .iter()
                .find(|(_, subtype)| name.ends_with(subtype))
        })
        .map(|(_, subtype)| (*subtype).to_string())
}

pub fn market_item(catalog: &Catalog, entry: &ItemEntry) -> Option<MarketItemDto> {
    catalog
        .resolve(&entry.unique_name)
        .or_else(|| {
            (entry.kind == "relic")
                .then(|| catalog.resolve(relic_base(&entry.unique_name)))
                .flatten()
        })
        .map(market::item_dto)
}

pub fn cards(db: &ItemDb, catalog: &Catalog) -> Vec<CatalogCardDto> {
    let mut cards: Vec<CatalogCardDto> = Vec::new();
    for entry in db.all() {
        if entry.kind == "relic" {
            continue;
        }
        let equipment = is_equipment(&entry.kind);
        let market = market_item(catalog, entry);
        let ayatan = market.as_ref().is_some_and(|item| is_ayatan(&item.tags));
        if !equipment && entry.kind != "mod" && entry.kind != "arcane" && !ayatan {
            continue;
        }
        cards.push(CatalogCardDto {
            item: item_dto(entry),
            market,
            components: if equipment {
                components(db, catalog, entry)
            } else {
                Vec::new()
            },
        });
    }
    cards.extend(
        catalog
            .all()
            .iter()
            .filter(|item| item.is_relic())
            .map(|item| CatalogCardDto {
                item: market_entry(item),
                market: Some(market::item_dto(item)),
                components: Vec::new(),
            }),
    );
    cards.sort_by(|left, right| {
        left.item
            .kind
            .cmp(&right.item.kind)
            .then_with(|| left.item.name.cmp(&right.item.name))
    });
    cards
}

pub fn status(db: Option<&ItemDb>, error: Option<String>) -> ItemDbStatusDto {
    let path = ItemDb::path();
    let age_secs = std::fs::metadata(&path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .map(|age| age.as_secs());
    ItemDbStatusDto {
        available: db.is_some(),
        path: path.display().to_string(),
        items: db.map_or(0, |db| db.all().len()),
        age_secs,
        fetched_at: db.map_or(0, |db| db.fetched_at),
        source: db.map_or_else(
            || coruframe_core::itemdb::SOURCE_NOTE.to_string(),
            |db| db.source.clone(),
        ),
        categories: db.map_or_else(BTreeMap::new, |db| db.counts.clone()),
        mastery_available: mastery_available(),
        error,
    }
}

// Mastery needs the profile XPInfo array; the scanner cannot capture it until
// the key is verified on a live build, so the inventory stays ownership-only.
pub fn mastery_available() -> bool {
    let path = fetch::cache_dir().join("inventory-raw.json");
    read_json(&path)
        .and_then(|value| value.get("XPInfo").map(serde_json::Value::is_array))
        .unwrap_or(false)
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_image_urls_only_for_safe_names() {
        assert_eq!(
            image_url(Some("AshPrime.png")).as_deref(),
            Some("https://cdn.warframestat.us/img/AshPrime.png")
        );
        assert!(image_url(None).is_none());
        assert!(image_url(Some("")).is_none());
        assert!(image_url(Some("../../etc/passwd")).is_none());
        assert!(image_url(Some("https://evil.example/x.png")).is_none());
        assert!(image_url(Some("with space.png")).is_none());
        assert!(image_url(Some("sub/dir.png")).is_none());
        assert!(image_url(Some("line\nbreak.png")).is_none());
        assert!(image_url(Some("NewLokaAmaryn'sRetreat.png")).is_some());
    }

    const WARFRAMES: &str = r#"[
      {"uniqueName":"/Lotus/Powersuits/Excalibur/Excalibur","name":"Excalibur","category":"Warframes","type":"Warframe",
       "masterable":true,"components":[
         {"uniqueName":"/R/ExcaliburBlueprint","itemCount":1},
         {"uniqueName":"/R/ExcaliburChassis","itemCount":1},
         {"uniqueName":"/Items/OrokinCell","itemCount":1}
       ]},
      {"uniqueName":"/Lotus/Powersuits/EntratiMech/NechroTech","name":"Voidrig","category":"Warframes","type":"Necramech",
       "masterable":true,"components":[
         {"uniqueName":"/R/NecromechBlueprint","itemCount":1},
         {"uniqueName":"/Items/NecromechPartSystemsItem","itemCount":1}
       ]}
    ]"#;

    const COMPONENTS: &str = r#"[
      {"uniqueName":"/R/ExcaliburBlueprint","name":"Blueprint","category":"Components",
       "parentUniqueNames":["/Lotus/Powersuits/Excalibur/Excalibur"]},
      {"uniqueName":"/R/ExcaliburChassis","name":"Chassis","category":"Components",
       "parentUniqueNames":["/Lotus/Powersuits/Excalibur/Excalibur"]},
      {"uniqueName":"/R/NecromechBlueprint","name":"Blueprint","category":"Components",
       "parentUniqueNames":["/Lotus/Powersuits/EntratiMech/NechroTech"]}
    ]"#;

    const RESOURCES: &str = r#"[
      {"uniqueName":"/Items/OrokinCell","name":"Orokin Cell","category":"Resources","type":"Resource"},
      {"uniqueName":"/Items/NecromechPartSystemsItem","name":"Voidrig Capsule","category":"Resources","type":"Resource","imageName":"GenericNecramechSystems.png"}
    ]"#;

    const DUP_FRAME: &str = r#"[
      {"uniqueName":"/Lotus/Powersuits/Dup/Dup","name":"Dup","category":"Warframes","type":"Warframe",
       "masterable":true,"components":[
         {"uniqueName":"/R/DupBlueprint","itemCount":1},
         {"uniqueName":"/R/DupHelmet","itemCount":1},
         {"uniqueName":"/R/OtherHelmet","itemCount":1}
       ]}
    ]"#;

    const DUP_COMPONENTS: &str = r#"[
      {"uniqueName":"/R/DupBlueprint","name":"Blueprint","category":"Components",
       "parentUniqueNames":["/Lotus/Powersuits/Dup/Dup"]},
      {"uniqueName":"/R/DupHelmet","name":"Neuroptics","category":"Components",
       "parentUniqueNames":["/Lotus/Powersuits/Dup/Dup"]},
      {"uniqueName":"/R/OtherHelmet","name":"Neuroptics","category":"Components",
       "parentUniqueNames":["/Lotus/Powersuits/Dup/Dup"]}
    ]"#;

    const MARKET: &str = r#"{"data":[
      {"slug":"voidrig_set","gameRef":"/Lotus/Powersuits/EntratiMech/NechroTech","tags":["necramech","set"],
       "i18n":{"en":{"name":"Voidrig Set"}}},
      {"slug":"voidrig_capsule","gameRef":"/Items/NecromechPartSystemsItem","tags":["necramech","component"],
       "i18n":{"en":{"name":"Voidrig Capsule","icon":"items/images/en/voidrig_capsule.abc.png"}}}
    ]}"#;

    const MODS: &str = r#"[
      {"uniqueName":"/Mods/Vitality","name":"Vitality","category":"Mods","tradable":true,
       "masterable":true,"fusionLimit":10}
    ]"#;

    const ARCANES: &str = r#"[
      {"uniqueName":"/Arcane/Energize","name":"Arcane Energize","category":"Arcanes","tradable":true,
       "masterable":true,"levelStats":[{},{},{},{},{},{}]}
    ]"#;

    const MISC: &str = r#"[
      {"uniqueName":"/Items/FusionTreasures/OroFusexF","name":"Ayatan Anasa Sculpture","category":"Misc",
       "type":"FusionTreasure"}
    ]"#;

    const MARKET_WIDE: &str = r#"{"data":[
      {"id":"m1","slug":"vitality","gameRef":"/Mods/Vitality","tags":["mod"],"maxRank":10,
       "subtypes":["regular"],"i18n":{"en":{"name":"Vitality"}}},
      {"id":"a1","slug":"arcane_energize","gameRef":"/Arcane/Energize","tags":["arcane_enhancement"],
       "bulkTradable":true,"maxRank":5,"i18n":{"en":{"name":"Arcane Energize"}}},
      {"id":"r1","slug":"axi_a5_relic","gameRef":"/Projections/T4VoidProjectionMagNovaVaultB",
       "tags":["relic"],"subtypes":["intact","radiant"],"vaulted":false,
       "i18n":{"en":{"name":"Axi A5 Relic"}}},
      {"id":"y1","slug":"ayatan_anasa_sculpture","gameRef":"/Items/FusionTreasures/OroFusexF",
       "tags":["ayatan_sculpture"],"bulkTradable":true,"maxAmberStars":2,"maxCyanStars":2,
       "i18n":{"en":{"name":"Ayatan Anasa Sculpture"}}}
    ]}"#;

    #[test]
    fn builds_mod_arcane_relic_and_ayatan_cards() {
        let db =
            ItemDb::from_categories(&[("Mods", MODS), ("Arcanes", ARCANES), ("Misc", MISC)], 0)
                .unwrap();
        let catalog = Catalog::from_json(MARKET_WIDE).unwrap();
        let cards = cards(&db, &catalog);

        let vitality = cards
            .iter()
            .find(|card| card.item.name == "Vitality")
            .unwrap();
        assert_eq!(vitality.item.kind, "mod");
        assert_eq!(
            vitality.market.as_ref().map(|item| item.slug.as_str()),
            Some("vitality")
        );
        assert!(vitality.components.is_empty());

        let arcane = cards
            .iter()
            .find(|card| card.item.name == "Arcane Energize")
            .unwrap();
        assert_eq!(arcane.item.kind, "arcane");
        assert_eq!(arcane.market.as_ref().unwrap().max_rank, Some(5));

        // WFCD has no single relic entry; the card comes from the market side.
        let relic = cards
            .iter()
            .find(|card| card.item.name == "Axi A5 Relic")
            .unwrap();
        assert_eq!(relic.item.kind, "relic");
        assert_eq!(relic.market.as_ref().unwrap().subtypes.len(), 2);

        // Ayatan is a market tag on a Misc entry, not an item kind.
        let ayatan = cards
            .iter()
            .find(|card| card.item.name == "Ayatan Anasa Sculpture")
            .unwrap();
        assert_eq!(ayatan.item.kind, "misc");
        assert!(ayatan.market.as_ref().unwrap().bulk_tradable);
        assert!(ayatan.components.is_empty());
    }

    const RELIC_TIERS: &str = r#"[
      {"uniqueName":"/Projections/AxiA1Bronze","name":"Axi A1 Intact","category":"Relics","type":"Relic"},
      {"uniqueName":"/Projections/AxiA1Silver","name":"Axi A1 Exceptional","category":"Relics","type":"Relic"},
      {"uniqueName":"/Projections/AxiA1Gold","name":"Axi A1 Flawless","category":"Relics","type":"Relic"},
      {"uniqueName":"/Projections/AxiA1Platinum","name":"Axi A1 Radiant","category":"Relics","type":"Relic"},
      {"uniqueName":"/Projections/RequiemEterna","name":"Requiem Eterna Relic","category":"Relics","type":"Relic"}
    ]"#;

    const RELIC_CATALOG: &str = r#"{"data":[
      {"id":"r1","slug":"axi_a1_relic","gameRef":"/Projections/AxiA1","tags":["relic"],
       "subtypes":["intact","exceptional","flawless","radiant"],"i18n":{"en":{"name":"Axi A1 Relic"}}},
      {"id":"r2","slug":"requiem_eterna_relic","gameRef":"/Projections/RequiemEterna",
       "tags":["relic"],"subtypes":["intact","exceptional","flawless","radiant"],
       "i18n":{"en":{"name":"Requiem Eterna Relic"}}}
    ]}"#;

    #[test]
    fn joins_every_relic_tier_to_one_market_family() {
        let db = ItemDb::from_categories(&[("Relics", RELIC_TIERS)], 0).unwrap();
        let catalog = Catalog::from_json(RELIC_CATALOG).unwrap();
        let expected = [
            ("intact", "axi_a1_relic"),
            ("exceptional", "axi_a1_relic"),
            ("flawless", "axi_a1_relic"),
            ("radiant", "axi_a1_relic"),
        ];
        for (subtype, slug) in expected {
            let entry = db
                .all()
                .iter()
                .find(|entry| relic_subtype(entry).as_deref() == Some(subtype))
                .unwrap();
            let market = market_item(&catalog, entry).unwrap();
            assert_eq!(market.slug, slug);
            assert_eq!(relic_base(&entry.unique_name), "/Projections/AxiA1");
        }
        let requiem = db
            .all()
            .iter()
            .find(|entry| entry.name == "Requiem Eterna Relic")
            .unwrap();
        assert_eq!(relic_subtype(requiem), None);
        assert_eq!(
            market_item(&catalog, requiem).unwrap().slug,
            "requiem_eterna_relic"
        );
    }

    #[test]
    fn derives_the_refinement_from_the_name_when_the_path_has_none() {
        let db = ItemDb::from_categories(
            &[(
                "Relics",
                r#"[{"uniqueName":"/Projections/Odd","name":"Neo O1 Radiant","category":"Relics"}]"#,
            )],
            0,
        )
        .unwrap();
        assert_eq!(relic_subtype(&db.all()[0]).as_deref(), Some("radiant"));
    }

    #[test]
    fn resolves_a_component_blueprint_listing() {
        let catalog = Catalog::from_json(
            r#"{"data":[{"slug":"ash_prime_chassis_blueprint","gameRef":"/R/AshPrimeChassisBlueprint",
                "tags":["component"],"i18n":{"en":{"name":"Ash Prime Chassis Blueprint"}}}]}"#,
        )
        .unwrap();
        assert_eq!(
            component_market(&catalog, "/R/AshPrimeChassisComponent")
                .map(|item| item.slug.as_str()),
            Some("ash_prime_chassis_blueprint")
        );
        assert!(component_market(&catalog, "/R/Unknown").is_none());
    }

    #[test]
    fn dedupes_components_with_the_same_display_name() {
        let db = ItemDb::from_categories(
            &[("Warframes", DUP_FRAME), ("Components", DUP_COMPONENTS)],
            0,
        )
        .unwrap();
        let catalog = Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let cards = cards(&db, &catalog);
        let dup = cards.iter().find(|card| card.item.name == "Dup").unwrap();
        let names: Vec<&str> = dup
            .components
            .iter()
            .map(|part| part.name.as_str())
            .collect();
        assert_eq!(names, ["Dup Blueprint", "Dup Neuroptics"]);
    }

    #[test]
    fn joins_market_and_filters_parts() {
        let db = ItemDb::from_categories(
            &[
                ("Warframes", WARFRAMES),
                ("Components", COMPONENTS),
                ("Resources", RESOURCES),
            ],
            0,
        )
        .unwrap();
        let catalog = Catalog::from_json(MARKET).unwrap();
        let cards = cards(&db, &catalog);

        let excalibur = cards
            .iter()
            .find(|card| card.item.name == "Excalibur")
            .unwrap();
        assert!(
            excalibur.market.is_none(),
            "base frames have no market entry"
        );
        assert_eq!(
            excalibur.item.mastery_cap,
            Some(coruframe_core::mastery::FRAME_RANK_30_XP)
        );
        let names: Vec<&str> = excalibur
            .components
            .iter()
            .map(|part| part.name.as_str())
            .collect();
        assert_eq!(names, ["Excalibur Blueprint", "Excalibur Chassis"]);

        let voidrig = cards
            .iter()
            .find(|card| card.item.name == "Voidrig")
            .unwrap();
        assert_eq!(
            voidrig.market.as_ref().map(|item| item.slug.as_str()),
            Some("voidrig_set")
        );
        // The blueprint is a Components entry; the capsule is a WFCD Resource
        // that only the market marks as a part.
        assert_eq!(voidrig.components.len(), 2);
        let capsule = voidrig
            .components
            .iter()
            .find(|part| part.name == "Voidrig Capsule")
            .unwrap();
        assert_eq!(
            capsule.image.as_deref(),
            Some("GenericNecramechSystems.png")
        );
    }
}
