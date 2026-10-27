use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::fetch;

pub const SOURCE_BASE: &str =
    "https://raw.githubusercontent.com/WFCD/warframe-items/master/data/json/";
pub const SOURCE_NOTE: &str =
    "WFCD/warframe-items (MIT); game data and images belong to Digital Extremes";
pub const VERSION: u32 = 5;

const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

// Order matters for duplicate resolution: equipment categories are inserted
// before the scraped Components/Misc dumps.
pub const CATEGORIES: [&str; 18] = [
    "Warframes",
    "Primary",
    "Secondary",
    "Melee",
    "Sentinels",
    "Pets",
    "SentinelWeapons",
    "Archwing",
    "Arch-Gun",
    "Arch-Melee",
    "Components",
    "Mods",
    "Arcanes",
    "Relics",
    "Resources",
    "Misc",
    "Gear",
    "Fish",
];

pub const EQUIPMENT_KINDS: [&str; 9] = [
    "warframe",
    "primary",
    "secondary",
    "melee",
    "companion",
    "companion_weapon",
    "amp",
    "archwing",
    "necramech",
];

pub const KITGUN_CHAMBERS: [&str; 6] = [
    "Catchmoon",
    "Gaze",
    "Rattleguts",
    "Tombfinger",
    "Sporelacer",
    "Vermisplicer",
];

pub fn is_kitgun_chamber(entry: &ItemEntry) -> bool {
    entry.category == "Misc" && KITGUN_CHAMBERS.contains(&entry.name.as_str())
}

pub fn is_equipment(kind: &str) -> bool {
    EQUIPMENT_KINDS.contains(&kind)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ComponentRef {
    #[serde(rename = "uniqueName")]
    pub unique_name: String,
    pub name: String,
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub part: bool,
    #[serde(default)]
    pub tradable: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemEntry {
    #[serde(rename = "uniqueName")]
    pub unique_name: String,
    pub name: String,
    pub category: String,
    pub kind: String,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default)]
    pub tradable: bool,
    #[serde(default)]
    pub masterable: bool,
    #[serde(
        rename = "masteryReq",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub mastery_req: Option<u64>,
    #[serde(rename = "isPrime", default)]
    pub prime: bool,
    #[serde(default)]
    pub vaulted: bool,
    #[serde(rename = "maxRank", default, skip_serializing_if = "Option::is_none")]
    pub max_rank: Option<u64>,
    #[serde(
        rename = "buildPrice",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub build_price: Option<u64>,
    #[serde(rename = "buildTime", default, skip_serializing_if = "Option::is_none")]
    pub build_time: Option<u64>,
    #[serde(
        rename = "buildQuantity",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub build_quantity: Option<u64>,
    #[serde(
        rename = "skipBuildTimePrice",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub skip_build_time_price: Option<u64>,
    #[serde(rename = "bpCost", default, skip_serializing_if = "Option::is_none")]
    pub bp_cost: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<ComponentRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDb {
    pub version: u32,
    pub source: String,
    pub fetched_at: u64,
    pub counts: BTreeMap<String, usize>,
    pub items: Vec<ItemEntry>,
    #[serde(skip)]
    by_unique: HashMap<String, usize>,
    #[serde(skip)]
    by_base: HashMap<String, usize>,
    #[serde(skip)]
    by_name: HashMap<String, usize>,
}

#[derive(Deserialize)]
struct RawItem {
    #[serde(rename = "uniqueName", default)]
    unique_name: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(rename = "type", default)]
    item_type: Option<String>,
    #[serde(rename = "imageName", default)]
    image_name: Option<String>,
    #[serde(default)]
    tradable: Option<bool>,
    #[serde(default)]
    masterable: Option<bool>,
    #[serde(rename = "masteryReq", default)]
    mastery_req: Option<u64>,
    #[serde(rename = "isPrime", default)]
    is_prime: Option<bool>,
    #[serde(default)]
    vaulted: Option<bool>,
    #[serde(rename = "fusionLimit", default)]
    fusion_limit: Option<u64>,
    #[serde(rename = "levelStats", default)]
    level_stats: Vec<serde_json::Value>,
    #[serde(rename = "buildPrice", default)]
    build_price: Option<u64>,
    #[serde(rename = "buildTime", default)]
    build_time: Option<u64>,
    #[serde(rename = "buildQuantity", default)]
    build_quantity: Option<u64>,
    #[serde(rename = "skipBuildTimePrice", default)]
    skip_build_time_price: Option<u64>,
    #[serde(rename = "bpCost", default)]
    bp_cost: Option<u64>,
    #[serde(default)]
    components: Vec<RawComponent>,
    #[serde(rename = "parentUniqueNames", default)]
    parent_unique_names: Vec<String>,
}

#[derive(Deserialize)]
struct RawComponent {
    #[serde(rename = "uniqueName")]
    unique_name: String,
    #[serde(rename = "itemCount", default = "one")]
    item_count: u64,
}

fn one() -> u64 {
    1
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn kind_of(category: &str, item_type: Option<&str>, masterable: bool) -> &'static str {
    // Sentinels, MOAs and beasts carry their weapons in Primary with this type;
    // they are companions in the arsenal, not player primaries.
    if item_type == Some("Companion Weapon") {
        return "companion_weapon";
    }
    match category {
        "Warframes" => match item_type {
            // The dataset has no Necramech category: Voidrig and Bonewidow live
            // in Warframes.json with type Necramech.
            Some("Necramech") => "necramech",
            Some("Warframe") if masterable => "warframe",
            _ => "misc",
        },
        "Primary" => "primary",
        "Secondary" => "secondary",
        "Melee" => "melee",
        "Sentinels" | "SentinelWeapons" => "companion",
        // Pets.json also carries Kubrow/Moa part scraps and resources.
        "Pets" if item_type == Some("Pets") => "companion",
        "Pets" => "component",
        "Archwing" | "Arch-Gun" | "Arch-Melee" => "archwing",
        "Components" => "component",
        "Mods" => "mod",
        "Arcanes" => "arcane",
        "Relics" => "relic",
        "Resources" => "resource",
        "Fish" => "fish",
        "Gear" => "gear",
        _ => "misc",
    }
}

// Kuva/Tenet/Coda weapons, Paracesis and Necramechs reach rank 40; everything
// else caps at 30. The mastery module turns these into XP thresholds.
fn equipment_max_rank(name: &str, kind: &str) -> u64 {
    if kind == "necramech"
        || name == "Paracesis"
        || name.starts_with("Kuva ")
        || name.starts_with("Tenet ")
        || name.starts_with("Coda ")
    {
        40
    } else {
        30
    }
}

fn precedence(kind: &str) -> u8 {
    if is_equipment(kind) {
        3
    } else if kind == "component" {
        2
    } else {
        1
    }
}

impl ItemDb {
    pub fn path() -> PathBuf {
        fetch::cache_dir().join("itemdb.json")
    }

    pub fn cache_name(category: &str) -> String {
        format!("itemdb-{}.json", category.to_ascii_lowercase())
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut db: Self =
            serde_json::from_str(text).map_err(|error| format!("itemdb json: {error}"))?;
        if db.version != VERSION {
            return Err(format!(
                "itemdb version {} is stale (want {VERSION}); run `coruframe itemdb update`",
                db.version
            ));
        }
        db.build_indexes();
        Ok(db)
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::path();
        let text = std::fs::read_to_string(&path).map_err(|error| {
            format!("{}: {error}; run `coruframe itemdb update`", path.display())
        })?;
        Self::from_json(&text)
    }

    pub fn build(force: bool) -> Result<Self, String> {
        let max_age = if force { Duration::ZERO } else { MAX_AGE };
        let mut docs: Vec<(&str, String)> = Vec::with_capacity(CATEGORIES.len());
        for category in CATEGORIES {
            let url = format!("{SOURCE_BASE}{category}.json");
            let name = Self::cache_name(category);
            let json = fetch::fetch(&url, &name, max_age)
                .map_err(|error| format!("{category}: {error}"))?;
            docs.push((category, json));
        }
        let refs: Vec<(&str, &str)> = docs
            .iter()
            .map(|(category, json)| (*category, json.as_str()))
            .collect();
        let db = Self::from_categories(&refs, now())?;
        db.save()?;
        Ok(db)
    }

    fn save(&self) -> Result<(), String> {
        let path = Self::path();
        let json = serde_json::to_vec(self).map_err(|error| format!("itemdb encode: {error}"))?;
        crate::private_fs::write_private(&path, &json)
            .map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn from_categories(categories: &[(&str, &str)], fetched_at: u64) -> Result<Self, String> {
        let mut entries = Vec::new();
        let mut counts = BTreeMap::new();
        for (category, json) in categories {
            let raw: Vec<RawItem> =
                serde_json::from_str(json).map_err(|error| format!("{category}: {error}"))?;
            let mut kept = 0usize;
            for item in raw {
                let (Some(unique_name), Some(name), Some(item_category)) = (
                    item.unique_name.filter(|value| !value.is_empty()),
                    item.name.filter(|value| !value.is_empty()),
                    item.category.filter(|value| !value.is_empty()),
                ) else {
                    eprintln!("itemdb: skipped an entry in {category} without uniqueName, name or category");
                    continue;
                };
                let item_type = item.item_type;
                let mut kind = kind_of(
                    &item_category,
                    item_type.as_deref(),
                    item.masterable.unwrap_or(false),
                );
                // The secondary son conjured by Sirius & Orion is not a frame
                // and awards no mastery, despite WFCD marking it masterable.
                if unique_name.ends_with("/OrionSuit") {
                    kind = "misc";
                }
                // Amp mastery is per prism; WFCD files the prisms under Misc.
                if kind == "misc" && item_type.as_deref() == Some("Amp") && name.ends_with("Prism")
                {
                    kind = "amp";
                }
                // PvP variants share the base item's mastery and would only
                // duplicate the card.
                if unique_name.contains("/PvPVariant") {
                    continue;
                }
                // Only a zaw blade is a melee weapon; handles and balances are
                // parts ("grips" in-game) and award no mastery.
                let zaw = item_type.as_deref() == Some("Zaw Component");
                if zaw {
                    kind = if unique_name.contains("/Tip/") || unique_name.contains("/Tips/") {
                        "melee"
                    } else {
                        "component"
                    };
                }
                // Modular parts carry the mastery of the built weapon even
                // though WFCD marks the part itself non-masterable.
                let masterable =
                    item.masterable.unwrap_or(false) || kind == "amp" || (zaw && kind == "melee");
                let max_rank = if is_equipment(kind) {
                    Some(equipment_max_rank(&name, kind))
                } else {
                    match item_category.as_str() {
                        "Mods" => item.fusion_limit,
                        "Arcanes" if !item.level_stats.is_empty() => {
                            Some(item.level_stats.len().saturating_sub(1) as u64)
                        }
                        _ => None,
                    }
                };
                entries.push(ItemEntry {
                    unique_name,
                    name,
                    category: item_category,
                    kind: kind.to_string(),
                    item_type,
                    image: item.image_name.filter(|value| !value.is_empty()),
                    tradable: item.tradable.unwrap_or(false),
                    masterable,
                    mastery_req: item.mastery_req,
                    prime: item.is_prime.unwrap_or(false),
                    vaulted: item.vaulted.unwrap_or(false),
                    max_rank,
                    build_price: item.build_price,
                    build_time: item.build_time,
                    build_quantity: item.build_quantity,
                    skip_build_time_price: item.skip_build_time_price,
                    bp_cost: item.bp_cost,
                    components: item
                        .components
                        .into_iter()
                        .map(|component| ComponentRef {
                            unique_name: component.unique_name,
                            count: component.item_count,
                            ..ComponentRef::default()
                        })
                        .collect(),
                    parent: item.parent_unique_names.into_iter().next(),
                });
                kept += 1;
            }
            counts.insert((*category).to_string(), kept);
        }

        let mut chosen: HashMap<String, usize> = HashMap::new();
        let mut deduped: Vec<ItemEntry> = Vec::with_capacity(entries.len());
        for entry in entries {
            match chosen.get(&entry.unique_name).copied() {
                Some(index) if precedence(&deduped[index].kind) >= precedence(&entry.kind) => {
                    eprintln!(
                        "itemdb: duplicate {} kept as {} (skipped {})",
                        entry.unique_name, deduped[index].kind, entry.kind
                    );
                }
                Some(index) => {
                    eprintln!(
                        "itemdb: duplicate {} promoted from {} to {}",
                        entry.unique_name, deduped[index].kind, entry.kind
                    );
                    deduped[index] = entry;
                }
                None => {
                    chosen.insert(entry.unique_name.clone(), deduped.len());
                    deduped.push(entry);
                }
            }
        }

        // Kitguns master by their firing chamber, but WFCD only ships the part.
        // Give each chamber a Primary and a Secondary entry so both arsenal
        // slots track mastery separately.
        let chambers: Vec<ItemEntry> = deduped
            .iter()
            .filter(|entry| is_kitgun_chamber(entry))
            .cloned()
            .collect();
        for chamber in chambers {
            for (slot, category, kind) in [
                ("primary", "Primary", "primary"),
                ("secondary", "Secondary", "secondary"),
            ] {
                deduped.push(ItemEntry {
                    unique_name: format!("{}#{slot}", chamber.unique_name),
                    name: chamber.name.clone(),
                    category: category.to_string(),
                    kind: kind.to_string(),
                    item_type: Some("Kitgun".to_string()),
                    image: chamber.image.clone(),
                    masterable: true,
                    max_rank: Some(30),
                    parent: Some(chamber.unique_name.clone()),
                    ..ItemEntry::default()
                });
            }
        }

        let mut db = Self {
            version: VERSION,
            source: SOURCE_NOTE.to_string(),
            fetched_at,
            counts,
            items: deduped,
            by_unique: HashMap::new(),
            by_base: HashMap::new(),
            by_name: HashMap::new(),
        };
        db.build_indexes();
        db.prefix_component_names();
        db.build_indexes();
        db.resolve_components();
        Ok(db)
    }

    fn build_indexes(&mut self) {
        self.by_unique.clear();
        self.by_base.clear();
        self.by_name.clear();
        for (index, item) in self.items.iter().enumerate() {
            self.by_unique
                .entry(item.unique_name.clone())
                .or_insert(index);
            self.by_base
                .entry(base(&item.unique_name).to_lowercase())
                .or_insert(index);
            self.by_name
                .entry(item.name.to_lowercase())
                .or_insert(index);
        }
    }

    // Components.json names are bare ("Carapace"), which reads badly in the
    // inventory; the parent equipment supplies the full name.
    fn prefix_component_names(&mut self) {
        let mut renames = Vec::new();
        for (index, item) in self.items.iter().enumerate() {
            if item.kind != "component" {
                continue;
            }
            let Some(parent_ref) = item.parent.as_deref() else {
                continue;
            };
            let Some(parent) = self.by_unique.get(parent_ref).copied() else {
                continue;
            };
            let parent_name = &self.items[parent].name;
            let lower = item.name.to_lowercase();
            if parent_name.is_empty() || lower.starts_with(&parent_name.to_lowercase()) {
                continue;
            }
            renames.push((index, format!("{parent_name} {}", item.name)));
        }
        for (index, name) in renames {
            self.items[index].name = name;
        }
    }

    fn resolve_components(&mut self) {
        // Resolve against every entry, not just Components.json: Necramech
        // parts live in Resources.json, and the app still wants their names.
        let lookup: HashMap<String, (String, bool, bool)> = self
            .items
            .iter()
            .map(|item| {
                (
                    item.unique_name.clone(),
                    (
                        item.name.clone(),
                        item.kind == "component" && item.parent.is_some(),
                        item.tradable,
                    ),
                )
            })
            .collect();
        for item in &mut self.items {
            if !is_equipment(&item.kind) {
                continue;
            }
            for component in &mut item.components {
                if let Some((name, part, tradable)) = lookup.get(&component.unique_name) {
                    component.name = name.clone();
                    component.part = *part;
                    component.tradable = *tradable;
                } else if component.name.is_empty() {
                    component.name = base(&component.unique_name).to_string();
                }
            }
        }
    }

    pub fn resolve(&self, query: &str) -> Option<&ItemEntry> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        self.by_unique
            .get(query)
            .or_else(|| self.by_base.get(&base(query).to_lowercase()))
            .or_else(|| self.by_name.get(&query.to_lowercase()))
            .map(|index| &self.items[*index])
    }

    pub fn all(&self) -> &[ItemEntry] {
        &self.items
    }

    pub fn equipment(&self) -> impl Iterator<Item = &ItemEntry> {
        self.items.iter().filter(|item| is_equipment(&item.kind))
    }

    pub fn parts<'a>(&self, item: &'a ItemEntry) -> impl Iterator<Item = &'a ComponentRef> + 'a {
        item.components.iter().filter(|component| component.part)
    }
}

// The reward card names a market item ("Hildryn Prime Systems Blueprint"); the
// item DB knows the set it belongs to and the component it resolves to. The
// payload and the HUD share this join so they cannot disagree.
pub fn find_reward_component<'a>(
    db: &'a ItemDb,
    reward: &str,
) -> Option<(&'a ItemEntry, &'a ComponentRef)> {
    if let Some(parent_path) = db.resolve(reward).and_then(|entry| entry.parent.as_deref()) {
        if let Some(parent) = db.resolve(parent_path) {
            if let Some(component) = match_component(&parent.components, reward) {
                return Some((parent, component));
            }
        }
    }
    db.equipment()
        .find_map(|parent| match_component(&parent.components, reward).map(|part| (parent, part)))
}

// A component blueprint ("Systems Blueprint") matches the built component
// ("Systems"); the main blueprint ("X Prime Blueprint") matches itself.
fn match_component<'a>(components: &'a [ComponentRef], reward: &str) -> Option<&'a ComponentRef> {
    let base = reward.strip_suffix(" Blueprint").unwrap_or(reward);
    components
        .iter()
        .find(|part| part.part && (part.name == reward || part.name == base))
}

pub fn is_main_blueprint(set_name: &str, component_name: &str) -> bool {
    component_name == format!("{set_name} Blueprint")
}

#[cfg(test)]
mod tests {
    use super::*;

    const WARFRAMES: &str = r#"[
      {"uniqueName":"/Lotus/Powersuits/Excalibur/Excalibur","name":"Excalibur","category":"Warframes","type":"Warframe",
       "imageName":"Excalibur.png","tradable":false,"masterable":true,"masteryReq":0,"isPrime":false,
       "buildPrice":25000,"buildTime":86400,"buildQuantity":1,"skipBuildTimePrice":50,"bpCost":35000,
       "components":[
         {"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/ExcaliburBlueprint","itemCount":1},
         {"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/ExcaliburChassisComponent","itemCount":1},
         {"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","itemCount":1}
       ]},
      {"uniqueName":"/Lotus/Powersuits/EntratiMech/NechroTech","name":"Voidrig","category":"Warframes","type":"Necramech",
       "imageName":"NechroMech.png","tradable":false,"masterable":true,"masteryReq":0,
       "components":[
         {"uniqueName":"/Lotus/Types/Recipes/DeimosRecipes/Mechs/NecromechBlueprint","itemCount":1},
         {"uniqueName":"/Lotus/Types/Gameplay/InfestedMicroplanet/Resources/Mechs/NecromechPartSystemsItem","itemCount":1}
       ]},
      {"uniqueName":"/Lotus/Powersuits/PowersuitAbilities/Helminth","name":"Helminth","category":"Warframes","type":"Warframe",
       "tradable":false,"masterable":false}
    ]"#;

    const COMPONENTS: &str = r#"[
      {"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/ExcaliburBlueprint","name":"Blueprint","category":"Components",
       "tradable":false,"masterable":false,"parentUniqueNames":["/Lotus/Powersuits/Excalibur/Excalibur"]},
      {"uniqueName":"/Lotus/Types/Recipes/WarframeRecipes/ExcaliburChassisComponent","name":"Chassis","category":"Components",
       "tradable":false,"masterable":false,"parentUniqueNames":["/Lotus/Powersuits/Excalibur/Excalibur"]},
      {"uniqueName":"/Lotus/Types/Recipes/SentinelRecipes/PrimeCarrierSentinelBlueprint","name":"Blueprint","category":"Components",
       "tradable":true,"parentUniqueNames":["/Lotus/Types/Sentinels/SentinelPowersuits/PrimeCarrierPowerSuit"]},
      {"uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/PrimeCarrierCarapace","name":"Carapace","category":"Components",
       "tradable":true,"parentUniqueNames":["/Lotus/Types/Sentinels/SentinelPowersuits/PrimeCarrierPowerSuit"]}
    ]"#;

    const SENTINELS: &str = r#"[
      {"uniqueName":"/Lotus/Types/Sentinels/SentinelPowersuits/PrimeCarrierPowerSuit","name":"Carrier Prime","category":"Sentinels",
       "type":"Sentinel","imageName":"CarrierPrime.png","tradable":false,"masterable":true,"isPrime":true,"vaulted":true,
       "components":[
         {"uniqueName":"/Lotus/Types/Recipes/SentinelRecipes/PrimeCarrierSentinelBlueprint","itemCount":1},
         {"uniqueName":"/Lotus/Types/Recipes/Weapons/WeaponParts/PrimeCarrierCarapace","itemCount":1},
         {"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","itemCount":6}
       ]}
    ]"#;

    const MODS: &str = r#"[
      {"uniqueName":"/Lotus/Upgrades/Mods/Warframe/AvatarAbilityDurationMod","name":"Continuity","category":"Mods",
       "type":"Warframe Mod","imageName":"Continuity.png","tradable":true,"masterable":true,"fusionLimit":10,
       "levelStats":[{"stats":["+5%"]}]}
    ]"#;

    const PRIMARY: &str = r#"[
      {"uniqueName":"/Lotus/Weapons/Tenno/Rifle/Rifle","name":"Braton","category":"Primary","type":"Rifle",
       "masterable":true,"buildPrice":25000,"buildTime":43200,"buildQuantity":1,"skipBuildTimePrice":0}
    ]"#;

    const RESOURCES: &str = r#"[
      {"uniqueName":"/Lotus/Types/Items/MiscItems/OrokinCell","name":"Orokin Cell","category":"Resources",
       "type":"Resource","imageName":"OrokinCell.png","tradable":false},
      {"uniqueName":"/Lotus/Types/Gameplay/InfestedMicroplanet/Resources/Mechs/NecromechPartSystemsItem",
       "name":"Voidrig Capsule","category":"Resources","type":"Resource","tradable":false}
    ]"#;

    const MISC_DUPLICATE: &str = r#"[
      {"uniqueName":"/Lotus/Powersuits/Excalibur/Excalibur","name":"Excalibur Duplicate","category":"Misc","type":"Misc"}
    ]"#;

    fn db() -> ItemDb {
        ItemDb::from_categories(
            &[
                ("Warframes", WARFRAMES),
                ("Sentinels", SENTINELS),
                ("Components", COMPONENTS),
                ("Mods", MODS),
                ("Resources", RESOURCES),
            ],
            0,
        )
        .unwrap()
    }

    #[test]
    fn maps_companion_weapons_out_of_primary() {
        assert_eq!(
            kind_of("Primary", Some("Companion Weapon"), true),
            "companion_weapon"
        );
        assert_eq!(kind_of("Primary", Some("Rifle"), true), "primary");
        assert!(is_equipment("companion_weapon"));
    }

    #[test]
    fn maps_amp_prisms_to_amp_equipment() {
        let db = ItemDb::from_categories(
            &[(
                "Misc",
                r#"[{"uniqueName":"/Lotus/AmpPrism","name":"Cantic Prism","category":"Misc",
                     "type":"Amp","imageName":"CanticPrism.png"}]"#,
            )],
            0,
        )
        .unwrap();
        let prism = db.resolve("/Lotus/AmpPrism").unwrap();
        assert_eq!(prism.kind, "amp");
        assert!(prism.masterable);
        assert_eq!(prism.max_rank, Some(30));
    }

    #[test]
    fn kitgun_chambers_get_both_arsenal_slots() {
        let db = ItemDb::from_categories(
            &[(
                "Misc",
                r#"[{"uniqueName":"/Lotus/Kitgun/Vermisplicer","name":"Vermisplicer",
                     "category":"Misc","type":"Pistol","imageName":"Vermisplicer.png"}]"#,
            )],
            0,
        )
        .unwrap();
        let primary = db.resolve("/Lotus/Kitgun/Vermisplicer#primary").unwrap();
        let secondary = db.resolve("/Lotus/Kitgun/Vermisplicer#secondary").unwrap();
        assert_eq!(primary.kind, "primary");
        assert_eq!(secondary.kind, "secondary");
        assert!(primary.masterable && secondary.masterable);
        assert_eq!(primary.name, "Vermisplicer");
        assert_eq!(
            primary.parent.as_deref(),
            Some("/Lotus/Kitgun/Vermisplicer")
        );
    }

    #[test]
    fn only_zaw_blades_stay_melee_equipment() {
        let db = ItemDb::from_categories(
            &[(
                "Melee",
                r#"[
                  {"uniqueName":"/Lotus/Weapons/Ostron/Melee/ModularMelee01/Tip/TipOne",
                   "name":"Balla","category":"Melee","type":"Zaw Component"},
                  {"uniqueName":"/Lotus/Weapons/Ostron/Melee/ModularMelee01/Handle/HandleFive",
                   "name":"Laka","category":"Melee","type":"Zaw Component"},
                  {"uniqueName":"/Lotus/Weapons/Ostron/Melee/ModularMelee01/Balance/BalanceSpeedIStatusII",
                   "name":"Vargeet II Jai","category":"Melee","type":"Zaw Component"},
                  {"uniqueName":"/Lotus/Weapons/Tenno/Melee/Skana",
                   "name":"Skana","category":"Melee","type":"Sword","masterable":true}
                ]"#,
            )],
            0,
        )
        .unwrap();
        let blade = db.resolve("Balla").unwrap();
        assert_eq!(blade.kind, "melee");
        assert!(blade.masterable);
        for part in ["Laka", "Vargeet II Jai"] {
            let entry = db.resolve(part).unwrap();
            assert_eq!(entry.kind, "component", "{part}");
            assert!(!entry.masterable, "{part}");
        }
        assert!(db.resolve("Skana").unwrap().masterable);
    }

    #[test]
    fn drops_pvp_variant_duplicates() {
        let db = ItemDb::from_categories(
            &[(
                "Primary",
                r#"[
                  {"uniqueName":"/Lotus/Weapons/Tenno/Rifle/Braton","name":"Braton",
                   "category":"Primary","type":"Rifle","masterable":true},
                  {"uniqueName":"/Lotus/Weapons/Tenno/Rifle/PvPVariant/Braton","name":"Braton",
                   "category":"Primary","type":"Rifle","masterable":true}
                ]"#,
            )],
            0,
        )
        .unwrap();
        let names: Vec<&str> = db
            .items
            .iter()
            .map(|item| item.unique_name.as_str())
            .collect();
        assert!(
            names.iter().all(|name| !name.contains("PvPVariant")),
            "{names:?}"
        );
        assert_eq!(
            db.items
                .iter()
                .filter(|item| item.kind == "primary")
                .count(),
            1
        );
    }

    #[test]
    fn maps_categories_and_necramechs() {
        let db = db();
        assert_eq!(db.resolve("Excalibur").unwrap().kind, "warframe");
        assert_eq!(db.resolve("Voidrig").unwrap().kind, "necramech");
        assert_eq!(db.resolve("Helminth").unwrap().kind, "misc");
        assert_eq!(db.resolve("Carrier Prime").unwrap().kind, "companion");
        assert_eq!(
            db.resolve("/Lotus/Types/Items/MiscItems/OrokinCell")
                .unwrap()
                .kind,
            "resource"
        );
        // Necramech parts live in Resources.json; the name still resolves, the
        // market decides whether they are part chips.
        let voidrig = db.resolve("Voidrig").unwrap();
        assert_eq!(
            voidrig.components[1].name, "Voidrig Capsule",
            "component names resolve across categories"
        );
        assert!(!voidrig.components[1].part);
        assert_eq!(db.equipment().count(), 3);
        assert_eq!(db.counts["Warframes"], 3);
        assert_eq!(db.counts["Components"], 4);
    }

    #[test]
    fn resolves_by_path_basename_and_name() {
        let db = db();
        for query in [
            "/Lotus/Powersuits/Excalibur/Excalibur",
            "/Lotus/StoreItems/Powersuits/Excalibur/Excalibur",
            "Excalibur",
        ] {
            assert_eq!(
                db.resolve(query).map(|item| item.name.as_str()),
                Some("Excalibur"),
                "query {query}"
            );
        }
        assert!(db.resolve("Nope").is_none());
    }

    #[test]
    fn equipment_wins_a_duplicate_unique_name() {
        let db = ItemDb::from_categories(&[("Warframes", WARFRAMES), ("Misc", MISC_DUPLICATE)], 0)
            .unwrap();
        assert_eq!(db.resolve("Excalibur").unwrap().name, "Excalibur");
        assert_eq!(db.resolve("Excalibur").unwrap().kind, "warframe");
        assert_eq!(
            db.all()
                .iter()
                .filter(|item| item.unique_name == "/Lotus/Powersuits/Excalibur/Excalibur")
                .count(),
            1
        );
    }

    #[test]
    fn resolves_and_filters_component_parts() {
        let db = db();
        let carrier = db.resolve("Carrier Prime").unwrap();
        let parts: Vec<&ComponentRef> = db.parts(carrier).collect();
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|part| part.part));
        assert!(parts
            .iter()
            .any(|part| part.name == "Carrier Prime Carapace"));
        // Orokin Cell is an ingredient, not a part chip.
        assert!(!carrier
            .components
            .iter()
            .any(|component| component.name == "Orokin Cell" && component.part));

        let excalibur = db.resolve("Excalibur").unwrap();
        let names: Vec<&str> = db.parts(excalibur).map(|part| part.name.as_str()).collect();
        assert_eq!(names, ["Excalibur Blueprint", "Excalibur Chassis"]);
    }

    #[test]
    fn the_secondary_son_is_not_a_separate_frame() {
        let doc = r#"[
          {"uniqueName":"/Lotus/Powersuits/SiriusOrion/SiriusSuit","name":"Sirius & Orion","category":"Warframes",
           "type":"Warframe","masterable":true,"productCategory":"SpecialItems"},
          {"uniqueName":"/Lotus/Powersuits/SiriusOrion/OrionSuit","name":"Orion & Sirius","category":"Warframes",
           "type":"Warframe","masterable":true,"productCategory":"SpecialItems"}
        ]"#;
        let db = ItemDb::from_categories(&[("Warframes", doc)], 0).unwrap();
        assert_eq!(db.resolve("Sirius & Orion").unwrap().kind, "warframe");
        assert_eq!(db.resolve("Orion & Sirius").unwrap().kind, "misc");
        assert_eq!(db.equipment().count(), 1);
    }

    #[test]
    fn keeps_build_metadata_for_frames_and_weapons() {
        let db = db();
        let excalibur = db.resolve("Excalibur").unwrap();
        assert_eq!(excalibur.build_price, Some(25000));
        assert_eq!(excalibur.build_time, Some(86400));
        assert_eq!(excalibur.build_quantity, Some(1));
        assert_eq!(excalibur.skip_build_time_price, Some(50));
        assert_eq!(excalibur.bp_cost, Some(35000));

        let db = ItemDb::from_categories(&[("Primary", PRIMARY)], 0).unwrap();
        let braton = db.resolve("Braton").unwrap();
        assert_eq!(braton.build_time, Some(43200));
        assert_eq!(braton.build_price, Some(25000));
        assert_eq!(braton.bp_cost, None);
    }

    #[test]
    fn a_component_blueprint_matches_its_component() {
        let parts = vec![
            ComponentRef {
                unique_name: "/p/Blueprint".into(),
                name: "Hildryn Prime Blueprint".into(),
                part: true,
                ..ComponentRef::default()
            },
            ComponentRef {
                unique_name: "/p/Systems".into(),
                name: "Hildryn Prime Systems".into(),
                part: true,
                ..ComponentRef::default()
            },
            ComponentRef {
                unique_name: "/p/Cell".into(),
                name: "Orokin Cell".into(),
                part: false,
                ..ComponentRef::default()
            },
        ];
        let matched = match_component(&parts, "Hildryn Prime Systems Blueprint").unwrap();
        assert_eq!(matched.name, "Hildryn Prime Systems");
        assert!(!is_main_blueprint("Hildryn Prime", &matched.name));
        let main = match_component(&parts, "Hildryn Prime Blueprint").unwrap();
        assert!(is_main_blueprint("Hildryn Prime", &main.name));
    }

    #[test]
    fn rejects_a_stale_cache_version() {
        let db = db();
        let mut doc: serde_json::Value = serde_json::to_value(&db).unwrap();
        doc["version"] = serde_json::json!(1);
        let error = ItemDb::from_json(&doc.to_string()).unwrap_err();
        assert!(error.contains("itemdb update"), "{error}");
    }

    #[test]
    fn keeps_rank_caps_for_mods_and_equipment() {
        let db = db();
        assert_eq!(db.resolve("Continuity").unwrap().max_rank, Some(10));
        assert_eq!(db.resolve("Excalibur").unwrap().max_rank, Some(30));
        assert_eq!(db.resolve("Voidrig").unwrap().max_rank, Some(40));
    }

    #[test]
    fn compact_round_trip_keeps_the_indexes() {
        let db = db();
        let json = serde_json::to_string(&db).unwrap();
        assert!(!json.contains("by_unique"));
        let restored = ItemDb::from_json(&json).unwrap();
        assert_eq!(
            restored
                .resolve("Carrier Prime Carapace")
                .map(|item| item.kind.clone()),
            Some("component".to_string())
        );
        assert_eq!(
            restored.resolve("Carrier Prime").unwrap().components.len(),
            3
        );
        assert_eq!(restored.counts, db.counts);
    }
}
