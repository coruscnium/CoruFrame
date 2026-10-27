use std::collections::BTreeMap;
use std::time::Duration;

use serde::Deserialize;

use crate::catalog::Catalog;
use crate::fetch;

pub const RELICS_URL: &str = "https://drops.warframestat.us/data/relics.json";
const MAX_AGE: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Deserialize, Clone)]
pub struct Entry {
    pub tier: String,
    #[serde(rename = "relicName", default)]
    pub name: String,
    pub state: String,
    pub rewards: Vec<Reward>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Reward {
    #[serde(rename = "itemName")]
    pub item_name: String,
    pub rarity: String,
    pub chance: f64,
}

#[derive(Deserialize)]
struct RelicsDoc {
    relics: Vec<Entry>,
}

pub fn from_json(json: &str) -> Result<Vec<Entry>, String> {
    let doc: RelicsDoc =
        serde_json::from_str(json).map_err(|error| format!("relics json: {error}"))?;
    Ok(doc.relics)
}

pub fn load(force: bool) -> Result<Vec<Entry>, String> {
    let max_age = if force { Duration::ZERO } else { MAX_AGE };
    let json = fetch::fetch(RELICS_URL, "relics.json", max_age)?;
    from_json(&json)
}

pub fn find<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let key = normalize(query);
    let full: Vec<&Entry> = entries
        .iter()
        .filter(|entry| normalize(&format!("{}{}", entry.tier, entry.name)) == key)
        .collect();
    if !full.is_empty() {
        return full;
    }
    entries
        .iter()
        .filter(|entry| normalize(&entry.name) == key)
        .collect()
}

// Inventory rows are named "Axi A1 Intact"; the drop table splits the same
// name into tier, relic name and state. One normal form lets them meet.
pub fn key(text: &str) -> String {
    normalize(text)
}

// Expected ducats per crack; rewards without a market ducat value count as zero.
pub fn expected_ducats(entry: &Entry, catalog: &Catalog) -> f64 {
    entry
        .rewards
        .iter()
        .filter_map(|reward| {
            let item = catalog.resolve(&reward.item_name)?;
            let ducats = item.ducats?;
            Some(reward.chance / 100.0 * ducats as f64)
        })
        .sum()
}

// Untradable rewards (Forma, resources) and tradable items with no online seller
// are worth zero platinum right now; a network failure is handled by the caller.
pub fn expected_plat(
    entry: &Entry,
    prices: &BTreeMap<String, Option<(u64, usize)>>,
) -> Option<f64> {
    let mut total = 0.0;
    for reward in &entry.rewards {
        if let Some((platinum, _)) = prices.get(&reward.item_name).copied().flatten() {
            total += reward.chance / 100.0 * platinum as f64;
        }
    }
    Some(total)
}

pub fn state_rank(state: &str) -> u8 {
    match state {
        "Intact" => 0,
        "Exceptional" => 1,
        "Flawless" => 2,
        "Radiant" => 3,
        _ => 4,
    }
}

fn normalize(text: &str) -> String {
    let mut key: String = text
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect();
    if let Some(stripped) = key.strip_suffix("relic") {
        key = stripped.to_string();
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELICS: &str = r#"{"relics":[
      {"tier":"Axi","relicName":"A5","state":"Intact","rewards":[
        {"itemName":"Fulmin Prime Barrel","rarity":"Uncommon","chance":25.0},
        {"itemName":"Fulmin Prime Stock","rarity":"Rare","chance":2.0}]},
      {"tier":"Axi","relicName":"A5","state":"Radiant","rewards":[
        {"itemName":"Fulmin Prime Barrel","rarity":"Uncommon","chance":16.67},
        {"itemName":"Fulmin Prime Stock","rarity":"Rare","chance":10.0}]}
    ]}"#;

    const ITEMS: &str = r#"{"data":[
      {"slug":"fulmin_prime_barrel","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/FulminPrimeBarrel","tags":["prime"],"ducats":15,
       "i18n":{"en":{"name":"Fulmin Prime Barrel"}}},
      {"slug":"fulmin_prime_stock","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/FulminPrimeStock","tags":["prime"],"ducats":45,
       "i18n":{"en":{"name":"Fulmin Prime Stock"}}}
    ]}"#;

    #[test]
    fn matches_by_tier_name_or_full_query() {
        let entries = from_json(RELICS).unwrap();
        for query in ["Axi A5", "axi_a5", "Axi A5 Relic", "A5"] {
            assert_eq!(find(&entries, query).len(), 2, "query {query}");
        }
        assert!(find(&entries, "Neo B1").is_empty());
    }

    #[test]
    fn inventory_names_share_a_key_with_drop_table_names() {
        assert_eq!(key("Axi A1 Intact"), key("axi  a1   intact"));
        assert_ne!(key("Axi A1 Intact"), key("Axi A1 Radiant"));
        assert_eq!(
            key("Axi A1 Radiant"),
            key(&format!("{} {} {}", "Axi", "A1", "Radiant"))
        );
    }

    #[test]
    fn expected_ducats_uses_chances_and_market_values() {
        let entries = from_json(RELICS).unwrap();
        let catalog = Catalog::from_json(ITEMS).unwrap();
        let intact = entries
            .iter()
            .find(|entry| entry.state == "Intact")
            .unwrap();
        let expected = 0.25 * 15.0 + 0.02 * 45.0;
        assert!((expected_ducats(intact, &catalog) - expected).abs() < 1e-9);
    }

    #[test]
    fn expected_plat_ignores_untradable_and_unlisted_rewards() {
        let entries = from_json(RELICS).unwrap();
        let intact = entries
            .iter()
            .find(|entry| entry.state == "Intact")
            .unwrap();
        let mut prices = BTreeMap::new();
        prices.insert("Fulmin Prime Barrel".to_string(), Some((10, 3)));
        let expected = 0.25 * 10.0;
        assert!((expected_plat(intact, &prices).unwrap() - expected).abs() < 1e-9);

        prices.insert("Fulmin Prime Stock".to_string(), None);
        assert!((expected_plat(intact, &prices).unwrap() - expected).abs() < 1e-9);

        prices.insert("Fulmin Prime Stock".to_string(), Some((100, 1)));
        let expected = 0.25 * 10.0 + 0.02 * 100.0;
        assert!((expected_plat(intact, &prices).unwrap() - expected).abs() < 1e-9);
    }
}
