use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::fetch;

pub const ITEMS_URL: &str = "https://api.warframe.market/v2/items";
pub const WORLDSTATE_URL: &str = "https://api.warframe.com/cdn/worldState.php";
const ITEMS_MAX_AGE: Duration = Duration::from_secs(24 * 3600);

#[derive(Debug, Deserialize, Clone)]
pub struct Item {
    #[serde(default)]
    pub id: String,
    pub slug: String,
    #[serde(rename = "gameRef")]
    pub game_ref: String,
    #[serde(default)]
    pub ducats: Option<u64>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub vaulted: Option<bool>,
    #[serde(default, rename = "setRoot")]
    pub set_root: Option<bool>,
    #[serde(default, rename = "setParts")]
    pub set_parts: Option<Vec<String>>,
    #[serde(default, rename = "quantityInSet")]
    pub quantity_in_set: Option<u64>,
    #[serde(default, rename = "maxRank")]
    pub max_rank: Option<u64>,
    #[serde(default, rename = "maxCharges")]
    pub max_charges: Option<u64>,
    #[serde(default, rename = "maxAmberStars")]
    pub max_amber_stars: Option<u64>,
    #[serde(default, rename = "maxCyanStars")]
    pub max_cyan_stars: Option<u64>,
    #[serde(default)]
    pub subtypes: Option<Vec<String>>,
    #[serde(default)]
    pub tradable: Option<bool>,
    #[serde(default, rename = "bulkTradable")]
    pub bulk_tradable: Option<bool>,
    #[serde(default, rename = "tradingTax")]
    pub trading_tax: Option<u64>,
    pub i18n: I18n,
}

#[derive(Debug, Deserialize, Clone)]
pub struct I18n {
    pub en: I18nEn,
}

#[derive(Debug, Deserialize, Clone)]
pub struct I18nEn {
    pub name: String,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub thumb: String,
}

impl Item {
    pub fn name(&self) -> &str {
        &self.i18n.en.name
    }

    pub fn is_relic(&self) -> bool {
        self.tags.iter().any(|tag| tag == "relic")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub slug: String,
    pub name: String,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

// Where a candidate came from. A full-catalog hit is never a free-for-all:
// it may only reach the high tier when it is exact, unique and clear of the
// runner-up (see `MatchEvidence::tier`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MatchSource {
    Local,
    #[default]
    Global,
    Full,
    Untradeable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatchEvidence {
    pub coverage: f32,
    pub anchor_len: u32,
    pub margin: f32,
    pub unique: bool,
    pub variant_agreement: f32,
    pub source: MatchSource,
    pub exact: bool,
}

impl Default for MatchEvidence {
    fn default() -> Self {
        Self {
            coverage: 0.0,
            anchor_len: 0,
            margin: 0.0,
            unique: true,
            variant_agreement: 1.0,
            source: MatchSource::Global,
            exact: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceTier {
    High,
    Medium,
    Low,
}

// The high tier is the only one allowed to draw a price. Coverage must be
// complete, the runner-up must be clearly behind, and the OCR variants must
// agree; everything else is card evidence plus `?`.
pub const HIGH_COVERAGE: f32 = 0.999;
pub const HIGH_MARGIN: f32 = 0.10;
pub const HIGH_AGREEMENT: f32 = 0.5;
pub const MEDIUM_COVERAGE: f32 = 0.74;

impl MatchEvidence {
    pub fn tier(&self) -> ConfidenceTier {
        let agreed = self.variant_agreement >= HIGH_AGREEMENT;
        if self.unique
            && self.coverage >= HIGH_COVERAGE
            && self.margin >= HIGH_MARGIN
            && agreed
            && (self.source != MatchSource::Full || self.exact)
        {
            ConfidenceTier::High
        } else if self.unique && self.coverage >= MEDIUM_COVERAGE && self.margin > 0.0 && agreed {
            ConfidenceTier::Medium
        } else {
            ConfidenceTier::Low
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchResult<'a> {
    pub item: &'a Item,
    pub evidence: MatchEvidence,
}

#[derive(Deserialize)]
struct ItemsDoc {
    data: Vec<Item>,
}

#[derive(Debug, Default, Clone)]
pub struct Catalog {
    items: Vec<Item>,
    by_id: HashMap<String, usize>,
    by_ref: HashMap<String, usize>,
    by_base: HashMap<String, usize>,
    by_slug: HashMap<String, usize>,
    by_name: HashMap<String, usize>,
}

// Slugs become URL path segments and cache names, so a remote catalog entry
// cannot choose `../`, a query, or whitespace.
pub fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 200
        && !slug.starts_with('.')
        && slug != ".."
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

impl Catalog {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let doc: ItemsDoc =
            serde_json::from_str(json).map_err(|error| format!("items json: {error}"))?;
        let items = doc
            .data
            .into_iter()
            .filter(|item| {
                if valid_slug(&item.slug) {
                    true
                } else {
                    eprintln!("catalog: skipping item with invalid slug {:?}", item.slug);
                    false
                }
            })
            .collect();
        Ok(Self::build(items))
    }

    fn build(items: Vec<Item>) -> Self {
        let mut catalog = Self {
            items,
            ..Self::default()
        };
        for (index, item) in catalog.items.iter().enumerate() {
            if !item.id.is_empty() {
                catalog.by_id.entry(item.id.clone()).or_insert(index);
            }
            catalog.by_ref.entry(item.game_ref.clone()).or_insert(index);
            catalog
                .by_base
                .entry(base(&item.game_ref).to_lowercase())
                .or_insert(index);
            catalog.by_slug.entry(item.slug.clone()).or_insert(index);
            catalog
                .by_name
                .entry(item.name().to_lowercase())
                .or_insert(index);
        }
        catalog
    }

    // The log carries `/Lotus/StoreItems/...` paths while the market carries
    // `/Lotus/Types/Recipes/...`; only the last segment is shared.
    pub fn resolve(&self, query: &str) -> Option<&Item> {
        let query = query.trim();
        let lowered = query.to_lowercase();
        self.by_ref
            .get(query)
            .or_else(|| self.by_base.get(&base(query).to_lowercase()))
            .or_else(|| self.by_slug.get(&lowered))
            .or_else(|| self.by_name.get(&lowered))
            .map(|index| &self.items[*index])
    }

    pub fn all(&self) -> &[Item] {
        &self.items
    }

    pub fn by_id(&self, id: &str) -> Option<&Item> {
        self.by_id.get(id).map(|index| &self.items[*index])
    }
}

// Tesseract emits Latin ligatures (U+FB01 .. U+FB06) that are alphabetic and
// survive the token filter whole. Folding the handful that occur in item names
// is the NFKC subset this pipeline needs; a full normalizer would be a new
// dependency for characters the game UI never renders.
fn fold_ligatures(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\u{FB00}' => out.push_str("ff"),
            '\u{FB01}' => out.push_str("fi"),
            '\u{FB02}' => out.push_str("fl"),
            '\u{FB03}' => out.push_str("ffi"),
            '\u{FB04}' => out.push_str("ffl"),
            '\u{FB05}' | '\u{FB06}' => out.push_str("st"),
            '\u{0131}' => out.push('i'),
            '\u{017F}' => out.push('s'),
            _ => out.extend(character.to_lowercase()),
        }
    }
    out
}

pub fn normalize_ocr(text: &str) -> String {
    fold_ligatures(text)
        .chars()
        .filter(|character| character.is_alphanumeric())
        .collect()
}

fn normalized(text: &str) -> String {
    normalize_ocr(text)
}

// A part of an item name, with the OCR confusions and Roman numerals that turn
// up in reward labels folded to a single form for comparison only.
fn canonical(token: &str) -> String {
    let token: String = token
        .chars()
        .map(|character| match character {
            '0' => 'o',
            '1' => 'l',
            '5' => 's',
            '8' => 'b',
            '9' => 'g',
            other => other,
        })
        .collect();
    match token.as_str() {
        "ii" => "2".to_string(),
        "iii" => "3".to_string(),
        "iv" => "4".to_string(),
        "vi" => "6".to_string(),
        "vii" => "7".to_string(),
        "viii" => "8".to_string(),
        "ix" => "9".to_string(),
        other => other.to_string(),
    }
}

impl Catalog {
    // Single-card matcher: the leading word anchors the item identity, then every
    // other word must appear somewhere in the crop text. This tolerates split
    // lines, a mangled "Blueprint" and a concatenated bottom label, while the
    // anchor stops a short name like "Bo Prime Blueprint" matching a suffix.
    // Pool-aware, with the evidence the confidence tiers need. A tie returns
    // None: a short anchor shared by two items must never be priced.
    pub fn best_reward_match_in(
        &self,
        text: &str,
        pool: Option<&HashSet<String>>,
        source: MatchSource,
    ) -> Option<MatchResult<'_>> {
        self.best_with(
            text,
            |item| pool.is_none_or(|pool| pool.contains(&item.slug)),
            source,
        )
    }

    fn best_with<F>(&self, text: &str, allow: F, source: MatchSource) -> Option<MatchResult<'_>>
    where
        F: Fn(&Item) -> bool,
    {
        let mut words = tokens(text);
        if words.is_empty() {
            return None;
        }
        let raw_words = words.clone();
        // The bottom card label sometimes OCRs as one concatenated word.
        words.push(normalized(text));
        let whole = normalized(text);

        let mut best: Option<(f32, usize, u32, bool, &Item)> = None;
        let mut second_coverage = 0.0f32;
        let mut second_covered = 0usize;
        let mut tied = false;
        for item in &self.items {
            if !allow(item) || item.name().ends_with(" Set") {
                continue;
            }
            let name = tokens(item.name());
            if name.len() < 2 || !words.iter().any(|word| anchor_matches(&name[0], word)) {
                continue;
            }
            let anchor_len = raw_words
                .iter()
                .filter(|word| anchor_matches(&name[0], word))
                .map(|word| word.chars().count() as u32)
                .max()
                .unwrap_or(0);
            let mut covered = 1;
            let mut identity_plus = false;
            for token in &name[1..] {
                if words.iter().any(|word| word_matches(token, word)) {
                    covered += 1;
                    if token != "prime" {
                        identity_plus = true;
                    }
                }
            }
            // First word plus "Prime" alone does not identify a part.
            if !identity_plus {
                continue;
            }
            let coverage = covered as f32 / name.len() as f32;
            let exact = !whole.is_empty() && whole.contains(&normalized(item.name()));
            // Equal coverage: the name with more words present is more specific
            // (Neuroptics Blueprint beats the base Blueprint).
            let better = best
                .as_ref()
                .is_none_or(|(best_coverage, best_covered, _, _, _)| {
                    coverage > *best_coverage + 1e-6
                        || ((coverage - *best_coverage).abs() <= 1e-6 && covered > *best_covered)
                });
            if better {
                if let Some((previous, previous_covered, _, _, _)) = &best {
                    second_coverage = second_coverage.max(*previous);
                    second_covered = second_covered.max(*previous_covered);
                }
                best = Some((coverage, covered, anchor_len, exact, item));
                tied = false;
            } else {
                second_coverage = second_coverage.max(coverage);
                second_covered = second_covered.max(covered);
                if best
                    .as_ref()
                    .is_some_and(|(best_coverage, best_covered, _, _, _)| {
                        (coverage - *best_coverage).abs() <= 1e-6 && covered == *best_covered
                    })
                {
                    tied = true;
                }
            }
        }
        if tied {
            return None;
        }
        best.map(|(coverage, covered, anchor_len, exact, item)| {
            // Equal coverage is not automatically ambiguous: the candidate that
            // covers more of the same name (Neuroptics Blueprint over Blueprint)
            // is the more specific reading. Only equal coverage *and* equal
            // covered tokens is a real tie.
            let coverage_gap = (coverage - second_coverage).max(0.0);
            let specificity_gap = if coverage_gap <= 1e-6 && covered > second_covered {
                (covered - second_covered) as f32 / covered as f32
            } else {
                0.0
            };
            MatchResult {
                item,
                evidence: MatchEvidence {
                    coverage,
                    anchor_len,
                    margin: if second_coverage <= 0.0 {
                        1.0
                    } else {
                        coverage_gap + specificity_gap
                    },
                    unique: true,
                    variant_agreement: 1.0,
                    source,
                    exact,
                },
            }
        })
    }
}

// Relic rewards that never appear in the market catalog: show the name, no price.
const UNTRADEABLE: &[&str] = &["Forma Blueprint", "Forma"];

pub fn untradeable(text: &str) -> Option<&'static str> {
    let lines: Vec<String> = text.lines().map(normalized).collect();
    let whole = normalized(text);
    // Tesseract mangles "Blueprint" and adds edge garbage ("FormaBIutleR-rigt",
    // "s ,f/ ___ Forma Blueprint"); no tradeable name contains "forma".
    if whole.contains("forma") {
        return Some("Forma Blueprint");
    }
    UNTRADEABLE.iter().find_map(|name| {
        let key = normalized(name);
        (whole == key || lines.contains(&key)).then_some(*name)
    })
}

// The first word decides identity, so it gets a stricter rule than the rest of
// the name: a two-edit gap once made "Braton Prime Barrel" tie with "Burston
// Prime Barrel". Prefix (a truncated OCR label) and containment still work.
fn anchor_matches(token: &str, word: &str) -> bool {
    if token == word {
        return true;
    }
    if word.chars().count() >= 3 && token.starts_with(word) {
        return true;
    }
    if token.chars().count() >= 4 && word.contains(token) {
        return true;
    }
    // A long name with one damaged glyph cluster ("Pharﬁasma" for Phantasma)
    // keeps its ends even when the middle is unreadable.
    let token_chars: Vec<char> = token.chars().collect();
    let word_chars: Vec<char> = word.chars().collect();
    if token_chars.len() >= 6
        && word_chars.len() >= 6
        && token_chars[..3] == word_chars[..3]
        && token_chars[token_chars.len() - 3..] == word_chars[word_chars.len() - 3..]
    {
        return true;
    }
    // Two edits are allowed only when the first three letters already agree, so
    // "Komgressq"/Kompressa survives while "Braton"/Burston stays separate.
    if token_chars.len() >= 5
        && word_chars.len() >= 5
        && token_chars.len().abs_diff(word_chars.len()) <= 2
        && token_chars[..3] == word_chars[..3]
        && edit_distance(token, word) <= 2
    {
        return true;
    }
    if edit_distance(token, word) <= 1 {
        return true;
    }
    let (left, right) = (canonical(token), canonical(word));
    edit_distance(&left, &right) <= 1
}

// OCR drops or swaps one character, and the bottom label can be one long word.
fn word_matches(token: &str, word: &str) -> bool {
    if token.chars().count() >= 4 && word.contains(token) {
        return true;
    }
    // A truncated trailing word ("recei", "bluei", the Kompressa card's
    // "Kom") still identifies the part.
    if word.chars().count() >= 3 && token.starts_with(word) {
        return true;
    }
    let allowed = match token.chars().count() {
        0..=3 => 0,
        4..=6 => 1,
        _ => 2,
    };
    if edit_distance(token, word) <= allowed {
        return true;
    }
    // Second pass only for digit/letter swaps and Roman numerals. It is a
    // comparison form, never the text that reaches the HUD.
    let (left, right) = (canonical(token), canonical(word));
    edit_distance(&left, &right) <= allowed
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (row, left_char) in left.chars().enumerate() {
        let mut current = vec![row + 1];
        for (column, right_char) in right.iter().enumerate() {
            let substitution = usize::from(left_char != *right_char);
            current.push(
                (previous[column] + substitution)
                    .min(previous[column + 1] + 1)
                    .min(current[column] + 1),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn tokens(text: &str) -> Vec<String> {
    fold_ligatures(text)
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect()
}

fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

pub fn load(force: bool) -> Result<Catalog, String> {
    let max_age = if force { Duration::ZERO } else { ITEMS_MAX_AGE };
    let json = fetch::fetch(ITEMS_URL, "items.json", max_age)?;
    Catalog::from_json(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_anchor_tie_is_not_guessed() {
        let catalog = Catalog::from_json(
            r#"{"data":[
              {"id":"1","slug":"kompressa_prime_blueprint","gameRef":"/p/KompressaPrimeBlueprint","tags":["prime"],"i18n":{"en":{"name":"Kompressa Prime Blueprint"}}},
              {"id":"2","slug":"komorex_prime_blueprint","gameRef":"/p/KomorexPrimeBlueprint","tags":["prime"],"i18n":{"en":{"name":"Komorex Prime Blueprint"}}}
            ]}"#,
        )
        .unwrap();
        assert!(catalog
            .best_reward_match_in("Kom Prime Blueprint", None, MatchSource::Full)
            .is_none());
        assert_eq!(
            catalog
                .best_reward_match_in("Komp Prime Blueprint", None, MatchSource::Full)
                .unwrap()
                .item
                .name(),
            "Kompressa Prime Blueprint"
        );
        assert_eq!(
            catalog
                .best_reward_match_in("Komorex Prime Blueprint", None, MatchSource::Full)
                .unwrap()
                .item
                .name(),
            "Komorex Prime Blueprint"
        );
    }

    #[test]
    fn a_three_letter_anchor_matches_a_truncated_name() {
        assert!(word_matches("kompressa", "kom"));
        assert!(word_matches("blueprint", "blu"));
        assert!(!word_matches("kompressa", "ko"));
    }

    #[test]
    fn the_anchor_does_not_absorb_a_different_item() {
        assert!(anchor_matches("kompressa", "kom"));
        assert!(anchor_matches("kompressa", "komgressq"));
        assert!(!anchor_matches("burston", "braton"));
        assert!(!anchor_matches("kompressa", "komorex"));
        assert!(anchor_matches("phantasma", "pharfiasma"));
    }

    const DOC: &str = r#"{"apiVersion":"0.25.0","data":[
      {"id":"1","slug":"fulmin_prime_barrel","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/FulminPrimeBarrel","tags":["prime","component","weapon"],"ducats":15,
       "i18n":{"en":{"name":"Fulmin Prime Barrel","icon":"x.png"}}},
      {"id":"2","slug":"axi_a5_relic","gameRef":"/Lotus/Types/Game/Projections/T4VoidProjectionMagNovaVaultB","tags":["relic","axi"],"subtypes":["intact"],"vaulted":false,
       "i18n":{"en":{"name":"Axi A5 Relic","icon":"y.png"}}}
    ]}"#;

    #[test]
    fn resolves_by_path_basename_slug_and_name() {
        let catalog = Catalog::from_json(DOC).unwrap();
        for query in [
            "/Lotus/Types/Recipes/Weapons/WeaponParts/FulminPrimeBarrel",
            "/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/FulminPrimeBarrel",
            "fulmin_prime_barrel",
            "Fulmin Prime Barrel",
        ] {
            assert_eq!(
                catalog.resolve(query).map(Item::name),
                Some("Fulmin Prime Barrel"),
                "query {query}"
            );
        }
        assert!(catalog.resolve("nope").is_none());
    }

    #[test]
    fn skips_items_with_unsafe_slugs() {
        let catalog = Catalog::from_json(
            r#"{"data":[
              {"id":"a","slug":"../../etc/passwd","gameRef":"/A","i18n":{"en":{"name":"A"}}},
              {"id":"b","slug":"with space","gameRef":"/B","i18n":{"en":{"name":"B"}}},
              {"id":"c","slug":"query?x","gameRef":"/C","i18n":{"en":{"name":"C"}}},
              {"id":"d","slug":".hidden","gameRef":"/D","i18n":{"en":{"name":"D"}}},
              {"id":"e","slug":"good_slug-1.2","gameRef":"/E","i18n":{"en":{"name":"E"}}}
            ]}"#,
        )
        .unwrap();
        assert_eq!(catalog.all().len(), 1);
        assert_eq!(catalog.all()[0].slug, "good_slug-1.2");
        assert!(valid_slug("axi_a1_relic"));
        assert!(valid_slug("arcane_energize"));
        assert!(!valid_slug(""));
        assert!(!valid_slug(".."));
        assert!(!valid_slug(".x"));
    }

    #[test]
    fn reads_ducats_and_tags() {
        let catalog = Catalog::from_json(DOC).unwrap();
        let barrel = catalog.resolve("FulminPrimeBarrel").unwrap();
        assert_eq!(barrel.ducats, Some(15));
        let relic = catalog.resolve("axi_a5_relic").unwrap();
        assert!(relic.is_relic());
        assert_eq!(relic.vaulted, Some(false));
    }

    #[test]
    fn reads_write_only_market_fields() {
        let catalog = Catalog::from_json(
            r#"{"data":[
              {"slug":"arcane_energize","gameRef":"/Arcane","tags":["arcane_enhancement"],
               "bulkTradable":true,"tradingTax":5000,"i18n":{"en":{"name":"Arcane Energize"}}}
            ]}"#,
        )
        .unwrap();
        let arcane = catalog.resolve("arcane_energize").unwrap();
        assert_eq!(arcane.bulk_tradable, Some(true));
        assert_eq!(arcane.trading_tax, Some(5000));
        let relic = Catalog::from_json(DOC).unwrap();
        assert_eq!(relic.resolve("axi_a5_relic").unwrap().bulk_tradable, None);
        assert_eq!(relic.resolve("axi_a5_relic").unwrap().trading_tax, None);
    }

    const REWARDS: &str = r#"{"data":[
      {"slug":"gyre_prime_neuroptics_blueprint","gameRef":"/Lotus/Types/Recipes/WarframeRecipes/GyrePrimeNeuropticsBlueprint","tags":["prime","component"],"ducats":45,
       "i18n":{"en":{"name":"Gyre Prime Neuroptics Blueprint"}}},
      {"slug":"velox_prime_blueprint","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/VeloxPrimeBlueprint","tags":["prime","component"],"ducats":45,
       "i18n":{"en":{"name":"Velox Prime Blueprint"}}},
      {"slug":"cedo_prime_blueprint","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/CedoPrimeBlueprint","tags":["prime","component"],"ducats":45,
       "i18n":{"en":{"name":"Cedo Prime Blueprint"}}},
      {"slug":"mesa_prime_blueprint","gameRef":"/Lotus/Types/Recipes/WarframeRecipes/MesaPrimeBlueprint","tags":["prime","component"],"ducats":15,
       "i18n":{"en":{"name":"Mesa Prime Blueprint"}}},
      {"slug":"mesa_prime_neuroptics_blueprint","gameRef":"/Lotus/Types/Recipes/WarframeRecipes/MesaPrimeNeuropticsBlueprint","tags":["prime","component"],"ducats":45,
       "i18n":{"en":{"name":"Mesa Prime Neuroptics Blueprint"}}},
      {"slug":"braton_prime_barrel","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/BratonPrimeBarrel","tags":["prime","component"],"ducats":15,
       "i18n":{"en":{"name":"Braton Prime Barrel"}}},
      {"slug":"braton_prime_set","gameRef":"/Lotus/Types/Recipes/Weapons/BratonPrimeSet","tags":["prime","set"],"ducats":60,
       "i18n":{"en":{"name":"Braton Prime Set"}}},
      {"slug":"epitaph_prime_receiver","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/EpitaphPrimeReceiver","tags":["prime","component"],"ducats":45,
       "i18n":{"en":{"name":"Epitaph Prime Receiver"}}},
      {"slug":"burston_prime_receiver","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/BurstonPrimeReceiver","tags":["prime","component"],"ducats":15,
       "i18n":{"en":{"name":"Burston Prime Receiver"}}},
      {"slug":"paris_prime_lower_limb","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/ParisPrimeLowerLimb","tags":["prime","component"],"ducats":15,
       "i18n":{"en":{"name":"Paris Prime Lower Limb"}}},
      {"slug":"dual_zoren_prime_blueprint","gameRef":"/Lotus/Types/Recipes/Weapons/WeaponParts/DualZorenPrimeBlueprint","tags":["prime","component"],"ducats":15,
       "i18n":{"en":{"name":"Dual Zoren Prime Blueprint"}}}
    ]}"#;

    #[test]
    fn reward_match_anchors_the_first_word() {
        let catalog = Catalog::from_json(
            r#"{"data":[
              {"slug":"bo_prime_blueprint","gameRef":"","tags":[],"ducats":15,
               "i18n":{"en":{"name":"Bo Prime Blueprint"}}},
              {"slug":"kompressa_prime_blueprint","gameRef":"","tags":[],"ducats":45,
               "i18n":{"en":{"name":"Kompressa Prime Blueprint"}}}
            ]}"#,
        )
        .unwrap();
        let text = "Komgressq Prime\nlueprint\nAPRESSA PRIME BLUEI";
        let found = catalog
            .best_reward_match_in(text, None, MatchSource::Full)
            .unwrap();
        assert_eq!(found.item.name(), "Kompressa Prime Blueprint");
        assert!(found.evidence.coverage > 0.99);
        let bo: HashSet<String> = ["bo_prime_blueprint".to_string()].into_iter().collect();
        assert!(catalog
            .best_reward_match_in(text, Some(&bo), MatchSource::Local)
            .is_none());
    }

    #[test]
    fn reward_match_reads_a_concatenated_label() {
        let catalog = Catalog::from_json(REWARDS).unwrap();
        assert_eq!(
            catalog
                .best_reward_match_in("DUALZORENPRIMEBLUEPRINT", None, MatchSource::Full)
                .map(|found| found.item.name()),
            Some("Dual Zoren Prime Blueprint")
        );
    }

    #[test]
    fn folds_ligatures_and_ocr_confusions() {
        assert_eq!(normalized("Kom‘gﬁsam"), "komgfisam");
        assert_eq!(normalized("Tenora Prime Blueptint"), "tenoraprimeblueptint");
        assert!(word_matches("blueprint", "blu3print"));
        assert!(word_matches("barrel", "barre1"));
        assert!(word_matches("systems", "5ystems"));
        assert!(word_matches("ii", "2"));
        // The confusion form is a comparison only; the original text is kept.
        assert_eq!(normalized("forma"), "forma");
    }

    #[test]
    fn high_needs_unique_exact_and_a_clear_margin() {
        let exact = MatchEvidence {
            coverage: 1.0,
            anchor_len: 6,
            margin: 0.5,
            unique: true,
            variant_agreement: 1.0,
            source: MatchSource::Global,
            exact: true,
        };
        assert_eq!(exact.tier(), ConfidenceTier::High);
        assert_eq!(
            MatchEvidence {
                margin: 0.05,
                ..exact.clone()
            }
            .tier(),
            ConfidenceTier::Medium
        );
        assert_eq!(
            MatchEvidence {
                variant_agreement: 0.25,
                ..exact.clone()
            }
            .tier(),
            ConfidenceTier::Low
        );
        assert_eq!(
            MatchEvidence {
                coverage: 0.75,
                ..exact.clone()
            }
            .tier(),
            ConfidenceTier::Medium
        );
        // A full-catalog hit without an exact name is never allowed a price.
        assert_eq!(
            MatchEvidence {
                source: MatchSource::Full,
                exact: false,
                ..exact
            }
            .tier(),
            ConfidenceTier::Medium
        );
    }

    #[test]
    fn the_matcher_reports_margin_and_anchor_length() {
        let catalog = Catalog::from_json(
            r#"{"data":[
              {"slug":"kompressa_prime_blueprint","gameRef":"","tags":[],"i18n":{"en":{"name":"Kompressa Prime Blueprint"}}},
              {"slug":"komorex_prime_blueprint","gameRef":"","tags":[],"i18n":{"en":{"name":"Komorex Prime Blueprint"}}}
            ]}"#,
        )
        .unwrap();
        assert!(catalog
            .best_reward_match_in("Kom Prime Blueprint", None, MatchSource::Full)
            .is_none());
        let found = catalog
            .best_reward_match_in("Komp Prime Blueprint", None, MatchSource::Global)
            .unwrap();
        assert_eq!(found.item.name(), "Kompressa Prime Blueprint");
        assert_eq!(found.evidence.anchor_len, 4);
        assert_eq!(found.evidence.margin, 1.0);
        assert_eq!(found.evidence.source, MatchSource::Global);
    }

    #[test]
    fn a_pool_only_matches_its_own_items() {
        let catalog = Catalog::from_json(REWARDS).unwrap();
        let pool: HashSet<String> = ["epitaph_prime_receiver".to_string()].into_iter().collect();
        assert!(catalog
            .best_reward_match_in("Braton Prime Barrel", Some(&pool), MatchSource::Local)
            .is_none());
        assert_eq!(
            catalog
                .best_reward_match_in("Epitaph Prime Receiver", Some(&pool), MatchSource::Local)
                .map(|found| found.evidence.source),
            Some(MatchSource::Local)
        );
    }

    #[test]
    fn recognizes_a_non_market_reward() {
        assert_eq!(untradeable("Forma Blueprint"), Some("Forma Blueprint"));
        assert_eq!(untradeable("FormaBlueprint"), Some("Forma Blueprint"));
        assert_eq!(untradeable("FormaBIutleR-rigt"), Some("Forma Blueprint"));
        assert_eq!(
            untradeable("s ,f/ ___ Forma Blueprint\nMA BLUEPRINT"),
            Some("Forma Blueprint")
        );
        assert!(untradeable("Braton Prime Barrel").is_none());
    }
}
