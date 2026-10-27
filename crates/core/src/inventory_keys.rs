use std::collections::HashMap;

use crate::fetch;

// One registry for every snapshot array. The scanner, the app and the frontend
// checklist all read from here; adding an array means updating this list and the
// places the snapshot manifest names.
pub const CORE: [&str; 6] = [
    "MiscItems",
    "Suits",
    "LongGuns",
    "Pistols",
    "Melee",
    "XPInfo",
];

pub const CATEGORY: [&str; 8] = [
    "Sentinels",
    "SentinelWeapons",
    "KubrowPets",
    "MechSuits",
    "SpaceSuits",
    "SpaceGuns",
    "SpaceMelee",
    "OperatorAmps",
];

// Mod ranks (`Upgrades`) and mod counts (`RawUpgrades`) gate the snapshot like
// the inventory arrays: without them every mod would silently render unranked
// or uncounted. An empty array is a real answer; an absent one is not. They are
// never synthesized or carried from a previous snapshot, so a scan that cannot
// extract them cannot pass the completeness check.
pub const RANKS: [&str; 2] = ["RawUpgrades", "Upgrades"];

pub const TRACKED: [&str; 16] = [
    "MiscItems",
    "Suits",
    "LongGuns",
    "Pistols",
    "Melee",
    "XPInfo",
    "Sentinels",
    "SentinelWeapons",
    "KubrowPets",
    "MechSuits",
    "SpaceSuits",
    "SpaceGuns",
    "SpaceMelee",
    "OperatorAmps",
    "RawUpgrades",
    "Upgrades",
];

pub fn is_tracked(key: &str) -> bool {
    TRACKED.contains(&key)
}

pub fn is_inventory(key: &str) -> bool {
    is_tracked(key)
}

// Owned counts keyed by the item's unique path. The raw snapshot is 0600 and
// optional: a missing file means the HUD/payload has no ownership data, not an
// error.
pub fn load_counts() -> Option<HashMap<String, u64>> {
    let text = std::fs::read_to_string(fetch::cache_dir().join("inventory-raw.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let mut counts = HashMap::new();
    for key in TRACKED {
        let Some(entries) = value.get(key).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for entry in entries {
            let Some(path) = entry.get("ItemType").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let count = entry
                .get("ItemCount")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            if count > 0 {
                *counts.entry(path.to_string()).or_insert(0) += count;
            }
        }
    }
    Some(counts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn tracked_has_no_duplicates_and_covers_every_array() {
        let unique: HashSet<&str> = TRACKED.iter().copied().collect();
        assert_eq!(unique.len(), TRACKED.len());
        let expected: HashSet<&str> = CORE
            .iter()
            .chain(CATEGORY.iter())
            .chain(RANKS.iter())
            .copied()
            .collect();
        assert_eq!(unique, expected);
    }
}
