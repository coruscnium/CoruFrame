use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::itemdb::{ItemDb, ItemEntry};

// Affinity to reach the cap, from the game's rank table:
// - Warframes, Companions, Archwings and Necramechs: 1000 x rank^2 (900,000 at 30)
// - Weapons: half of that (450,000 at 30)
// - Rank 40 overlevels: 800,000 for weapons, 1,600,000 for Necramechs.
pub const FRAME_RANK_30_XP: u64 = 900_000;
pub const WEAPON_RANK_30_XP: u64 = 450_000;
pub const WEAPON_RANK_40_XP: u64 = 800_000;
pub const NECRAMECH_RANK_40_XP: u64 = 1_600_000;

pub fn max_xp(entry: &ItemEntry) -> u64 {
    let overleveled = entry.max_rank.unwrap_or(30) >= 40;
    match entry.category.as_str() {
        "Warframes" if entry.kind == "necramech" => {
            if overleveled {
                NECRAMECH_RANK_40_XP
            } else {
                FRAME_RANK_30_XP
            }
        }
        "Warframes" | "Sentinels" | "Pets" | "Archwing" => FRAME_RANK_30_XP,
        "Primary" | "Secondary" | "Melee" | "SentinelWeapons" | "Arch-Gun" | "Arch-Melee" => {
            if overleveled {
                WEAPON_RANK_40_XP
            } else {
                WEAPON_RANK_30_XP
            }
        }
        _ => WEAPON_RANK_30_XP,
    }
}

// XPInfo is the profile's per-item affinity history: it still holds items that
// were mastered and sold, which the inventory snapshot cannot see.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct Mastery {
    #[serde(default)]
    pub xp: HashMap<String, u64>,
    #[serde(default)]
    pub mastered: Vec<String>,
}

impl Mastery {
    pub fn from_profile(profile: &serde_json::Value, db: &ItemDb) -> Self {
        let mut mastery = Self::default();
        let Some(entries) = profile.get("XPInfo").and_then(serde_json::Value::as_array) else {
            return mastery;
        };
        let primary = slot_chambers(profile, "LongGuns");
        let secondary = slot_chambers(profile, "Pistols");
        let mut mastered = HashSet::new();
        for entry in entries {
            let (Some(path), Some(xp)) = (
                entry.get("ItemType").and_then(serde_json::Value::as_str),
                entry.get("XP").and_then(serde_json::Value::as_u64),
            ) else {
                continue;
            };
            let Some(item) = db.resolve(path) else {
                continue;
            };
            // A kitgun's XP lives on its chamber, while the inventory stores the
            // built weapon under a generic path. The slot comes from the array
            // and the chamber from ModularParts; a sold chamber has neither, so
            // it badges both slots.
            if crate::itemdb::is_kitgun_chamber(item) {
                let mut slots = Vec::new();
                if primary.contains(item.unique_name.as_str()) {
                    slots.push("primary");
                }
                if secondary.contains(item.unique_name.as_str()) {
                    slots.push("secondary");
                }
                if slots.is_empty() {
                    slots.extend(["primary", "secondary"]);
                }
                for slot in slots {
                    let key = format!("{}#{slot}", item.unique_name);
                    mastery.xp.insert(key.clone(), xp);
                    if xp >= max_xp(item) {
                        mastered.insert(key);
                    }
                }
                continue;
            }
            if !crate::itemdb::is_equipment(&item.kind) {
                continue;
            }
            mastery.xp.insert(item.unique_name.clone(), xp);
            if xp >= max_xp(item) {
                mastered.insert(item.unique_name.clone());
            }
        }
        mastery.mastered = mastered.into_iter().collect();
        mastery.mastered.sort();
        mastery
    }

    pub fn is_mastered(&self, unique_name: &str) -> bool {
        self.mastered.iter().any(|name| name == unique_name)
    }
}

// Built kitguns are generic entries (`LotusModularPrimaryShotgun`) whose
// ModularParts name the chamber. A chamber stored directly as ItemType also
// counts, so both shapes of the profile work.
fn slot_chambers(profile: &serde_json::Value, key: &str) -> HashSet<String> {
    profile
        .get(key)
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            let mut paths = HashSet::new();
            for entry in entries {
                if let Some(path) = entry.get("ItemType").and_then(serde_json::Value::as_str) {
                    paths.insert(path.to_string());
                }
                if let Some(parts) = entry
                    .get("ModularParts")
                    .and_then(serde_json::Value::as_array)
                {
                    for part in parts.iter().filter_map(serde_json::Value::as_str) {
                        paths.insert(part.to_string());
                    }
                }
            }
            paths
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEMS: &str = r#"[
      {"uniqueName":"/P/Excalibur","name":"Excalibur","category":"Warframes","type":"Warframe","masterable":true},
      {"uniqueName":"/P/Voidrig","name":"Voidrig","category":"Warframes","type":"Necramech","masterable":true},
      {"uniqueName":"/W/Braton","name":"Braton","category":"Primary","type":"Rifle","masterable":true},
      {"uniqueName":"/W/KuvaKohm","name":"Kuva Kohm","category":"Primary","type":"Shotgun","masterable":true},
      {"uniqueName":"/W/CodaBassocyst","name":"Coda Bassocyst","category":"Primary","type":"Shotgun","masterable":true},
      {"uniqueName":"/S/Carrier","name":"Carrier","category":"Sentinels","type":"Sentinel","masterable":true},
      {"uniqueName":"/A/Archwing","name":"Odonata","category":"Archwing","type":"Archwing","masterable":true},
      {"uniqueName":"/A/KuvaGrattler","name":"Kuva Grattler","category":"Arch-Gun","type":"Arch-Gun","masterable":true}
    ]"#;

    fn db() -> ItemDb {
        ItemDb::from_categories(&[("Warframes", ITEMS)], 0).unwrap()
    }

    #[test]
    fn uses_the_class_specific_caps() {
        let profile = serde_json::json!({
            "XPInfo": [
                // Frame and companion cap at 900,000.
                {"ItemType": "/P/Excalibur", "XP": FRAME_RANK_30_XP},
                {"ItemType": "/S/Carrier", "XP": FRAME_RANK_30_XP - 1},
                {"ItemType": "/A/Archwing", "XP": FRAME_RANK_30_XP},
                // Weapons cap at 450,000.
                {"ItemType": "/W/Braton", "XP": WEAPON_RANK_30_XP},
                // Rank 40 weapons need 800,000; 648,000 is only rank ~36.
                {"ItemType": "/W/KuvaKohm", "XP": 648_000},
                {"ItemType": "/W/CodaBassocyst", "XP": WEAPON_RANK_40_XP},
                {"ItemType": "/A/KuvaGrattler", "XP": WEAPON_RANK_30_XP},
                // Necramechs are frames but need 1,600,000 at rank 40.
                {"ItemType": "/P/Voidrig", "XP": FRAME_RANK_30_XP}
            ]
        });
        let mastery = Mastery::from_profile(&profile, &db());
        assert!(mastery.is_mastered("/P/Excalibur"));
        assert!(!mastery.is_mastered("/S/Carrier"));
        assert!(mastery.is_mastered("/A/Archwing"));
        assert!(mastery.is_mastered("/W/Braton"));
        assert!(!mastery.is_mastered("/W/KuvaKohm"));
        assert!(mastery.is_mastered("/W/CodaBassocyst"));
        assert!(!mastery.is_mastered("/A/KuvaGrattler"));
        assert!(!mastery.is_mastered("/P/Voidrig"));
    }

    #[test]
    fn stops_one_xp_short_of_each_cap() {
        let profile = serde_json::json!({
            "XPInfo": [
                {"ItemType": "/P/Excalibur", "XP": FRAME_RANK_30_XP - 1},
                {"ItemType": "/W/Braton", "XP": WEAPON_RANK_30_XP - 1},
                {"ItemType": "/W/KuvaKohm", "XP": WEAPON_RANK_40_XP - 1},
                {"ItemType": "/P/Voidrig", "XP": NECRAMECH_RANK_40_XP - 1}
            ]
        });
        let mastery = Mastery::from_profile(&profile, &db());
        for name in ["/P/Excalibur", "/W/Braton", "/W/KuvaKohm", "/P/Voidrig"] {
            assert!(!mastery.is_mastered(name), "{name}");
        }
    }

    #[test]
    fn kitgun_mastery_follows_the_inventory_slot() {
        let db = ItemDb::from_categories(
            &[(
                "Misc",
                r#"[{"uniqueName":"/Lotus/Kitgun/Vermisplicer","name":"Vermisplicer",
                     "category":"Misc","type":"Pistol"}]"#,
            )],
            0,
        )
        .unwrap();
        let xp = WEAPON_RANK_30_XP;
        let profile = |long_guns: bool, pistols: bool| {
            let mut value = serde_json::json!({
                "XPInfo": [
                    {"ItemType": "/Lotus/Kitgun/Vermisplicer", "XP": xp}
                ],
                "LongGuns": [],
                "Pistols": []
            });
            if long_guns {
                value["LongGuns"] =
                    serde_json::json!([{"ItemType": "/Lotus/Kitgun/Vermisplicer", "ItemCount": 1}]);
            }
            if pistols {
                value["Pistols"] =
                    serde_json::json!([{"ItemType": "/Lotus/Kitgun/Vermisplicer", "ItemCount": 1}]);
            }
            value
        };

        let primary = Mastery::from_profile(&profile(true, false), &db);
        assert!(primary.is_mastered("/Lotus/Kitgun/Vermisplicer#primary"));
        assert!(!primary.is_mastered("/Lotus/Kitgun/Vermisplicer#secondary"));

        let secondary = Mastery::from_profile(&profile(false, true), &db);
        assert!(!secondary.is_mastered("/Lotus/Kitgun/Vermisplicer#primary"));
        assert!(secondary.is_mastered("/Lotus/Kitgun/Vermisplicer#secondary"));

        let both = Mastery::from_profile(&profile(true, true), &db);
        assert!(both.is_mastered("/Lotus/Kitgun/Vermisplicer#primary"));
        assert!(both.is_mastered("/Lotus/Kitgun/Vermisplicer#secondary"));

        // Sold in both slots: keep the mastery, badge both, own neither.
        let sold = Mastery::from_profile(&profile(false, false), &db);
        assert!(sold.is_mastered("/Lotus/Kitgun/Vermisplicer#primary"));
        assert!(sold.is_mastered("/Lotus/Kitgun/Vermisplicer#secondary"));

        // The real profile stores the built kitgun under a generic path and
        // names its chamber in ModularParts.
        let built = serde_json::json!({
            "XPInfo": [
                {"ItemType": "/Lotus/Kitgun/Vermisplicer", "XP": xp}
            ],
            "LongGuns": [{
                "ItemType": "/Lotus/Weapons/SolarisUnited/Primary/LotusModularPrimaryShotgun",
                "ModularParts": ["/Lotus/Kitgun/Vermisplicer"]
            }],
            "Pistols": []
        });
        let built = Mastery::from_profile(&built, &db);
        assert!(built.is_mastered("/Lotus/Kitgun/Vermisplicer#primary"));
        assert!(!built.is_mastered("/Lotus/Kitgun/Vermisplicer#secondary"));
    }

    #[test]
    fn tolerates_a_missing_profile_array() {
        let mastery = Mastery::from_profile(&serde_json::json!({}), &db());
        assert!(mastery.xp.is_empty());
        assert!(mastery.mastered.is_empty());
    }
}
