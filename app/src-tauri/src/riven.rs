use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use coruframe_core::fetch;

use crate::inventory;

const WEAPONS_URL: &str = "https://api.warframe.market/v2/riven/weapons";
const ATTRIBUTES_URL: &str = "https://api.warframe.market/v2/riven/attributes";
const LIST_TTL: Duration = Duration::from_secs(24 * 3_600);

#[derive(Deserialize)]
struct Doc<T> {
    data: Vec<T>,
}

#[derive(Deserialize, Clone)]
struct Lang {
    #[serde(default)]
    name: String,
    #[serde(default)]
    thumb: Option<String>,
}

#[derive(Deserialize, Clone, Default)]
struct I18n {
    en: Option<Lang>,
}

impl I18n {
    fn name(&self) -> Option<&str> {
        self.en.as_ref().map(|lang| lang.name.as_str())
    }

    fn thumb(&self) -> Option<&str> {
        self.en
            .as_ref()
            .and_then(|lang| lang.thumb.as_deref())
            .filter(|thumb| !thumb.is_empty())
    }
}

#[derive(Deserialize, Clone)]
struct Weapon {
    slug: String,
    #[serde(rename = "gameRef")]
    game_ref: String,
    #[serde(default, rename = "rivenType")]
    riven_type: Option<String>,
    #[serde(default)]
    i18n: I18n,
}

#[derive(Deserialize, Clone)]
struct Attribute {
    slug: String,
    #[serde(rename = "gameRef")]
    game_ref: String,
    #[serde(default)]
    unit: Option<String>,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
    #[serde(default)]
    i18n: I18n,
}

#[derive(Serialize, Clone)]
pub struct RivenStatDto {
    pub name: String,
    pub slug: String,
    pub value: f64,
    pub unit: String,
    pub positive: bool,
}

#[derive(Serialize, Clone)]
pub struct RivenDto {
    pub id: String,
    pub weapon: Option<String>,
    pub weapon_slug: Option<String>,
    pub weapon_type: Option<String>,
    pub weapon_thumb: Option<String>,
    pub polarity: String,
    pub rank: u64,
    pub mastery: Option<u64>,
    pub rerolls: u64,
    pub veiled: bool,
    pub challenge: Option<String>,
    pub stats: Vec<RivenStatDto>,
    // Generated from the rolls, the way the game names a riven.
    pub name: String,
}

#[derive(Serialize, Clone)]
pub struct RivenListingStatDto {
    pub name: String,
    pub url_name: String,
    pub value: f64,
    pub unit: String,
    pub positive: bool,
}

#[derive(Serialize, Clone)]
pub struct RivenListingDto {
    pub id: String,
    pub direct: bool,
    pub starting_price: u64,
    pub buyout_price: Option<u64>,
    pub top_bid: Option<u64>,
    pub created: String,
    pub visible: bool,
    pub note: String,
    pub name: String,
    pub weapon: Option<String>,
    pub weapon_slug: String,
    pub weapon_thumb: Option<String>,
    pub rank: Option<u64>,
    pub rerolls: u64,
    pub mastery: Option<u64>,
    pub polarity: Option<String>,
    pub stats: Vec<RivenListingStatDto>,
}

#[derive(Deserialize, Default)]
struct Fingerprint {
    #[serde(default)]
    compat: Option<String>,
    #[serde(default)]
    lvl: Option<u64>,
    #[serde(default, rename = "lvlReq")]
    lvl_req: Option<u64>,
    #[serde(default)]
    rerolls: Option<u64>,
    #[serde(default)]
    pol: Option<String>,
    #[serde(default)]
    buffs: Vec<Buff>,
    #[serde(default)]
    curses: Vec<Buff>,
    #[serde(default)]
    challenge: Option<Value>,
}

#[derive(Deserialize, Default)]
struct Buff {
    #[serde(default, rename = "Tag")]
    tag: String,
    #[serde(default, rename = "Value")]
    value: u64,
}

#[derive(Deserialize)]
struct ProfileDoc {
    payload: ProfilePayload,
}

#[derive(Deserialize)]
struct ProfilePayload {
    #[serde(default)]
    auctions: Vec<Auction>,
}

#[derive(Deserialize)]
struct Auction {
    id: String,
    #[serde(default)]
    starting_price: u64,
    #[serde(default)]
    buyout_price: Option<u64>,
    #[serde(default)]
    top_bid: Option<u64>,
    #[serde(default)]
    is_direct_sell: bool,
    #[serde(default)]
    visible: bool,
    #[serde(default)]
    closed: bool,
    #[serde(default)]
    note: String,
    #[serde(default)]
    created: String,
    item: AuctionItem,
}

#[derive(Deserialize)]
struct AuctionItem {
    #[serde(default)]
    name: String,
    #[serde(default)]
    weapon_url_name: String,
    #[serde(default)]
    mod_rank: Option<u64>,
    #[serde(default)]
    re_rolls: Option<u64>,
    #[serde(default)]
    mastery_level: Option<u64>,
    #[serde(default)]
    polarity: Option<String>,
    #[serde(default)]
    attributes: Vec<AuctionAttribute>,
}

#[derive(Deserialize)]
struct AuctionAttribute {
    #[serde(default)]
    value: f64,
    #[serde(default)]
    positive: bool,
    #[serde(default)]
    url_name: String,
}

fn fingerprint(entry: &Value) -> Option<Fingerprint> {
    match entry.get("UpgradeFingerprint")? {
        Value::String(text) => serde_json::from_str(text).ok(),
        other => serde_json::from_value(other.clone()).ok(),
    }
}

fn weapons_by_game_ref() -> HashMap<String, Weapon> {
    let Ok(text) = fetch::fetch(WEAPONS_URL, "riven-weapons.json", LIST_TTL) else {
        return HashMap::new();
    };
    let Ok(doc) = serde_json::from_str::<Doc<Weapon>>(&text) else {
        return HashMap::new();
    };
    doc.data
        .into_iter()
        .map(|weapon| (weapon.game_ref.clone(), weapon))
        .collect()
}

fn attributes() -> Vec<Attribute> {
    let Ok(text) = fetch::fetch(ATTRIBUTES_URL, "riven-attributes.json", LIST_TTL) else {
        return Vec::new();
    };
    serde_json::from_str::<Doc<Attribute>>(&text)
        .map(|doc| doc.data)
        .unwrap_or_default()
}

fn polarity(code: Option<&str>) -> String {
    match code {
        Some("AP_ATTACK") => "Madurai",
        Some("AP_DEFENSE") => "Vazarin",
        Some("AP_TACTIC") => "Naramon",
        Some(other) => other,
        None => "—",
    }
    .to_string()
}

fn weapon_class(item_type: &str) -> Option<&'static str> {
    if item_type.contains("LotusRifleRandomMod") {
        Some("Rifle")
    } else if item_type.contains("LotusPistolRandomMod") {
        Some("Pistol")
    } else if item_type.contains("LotusShotgunRandomMod") {
        Some("Shotgun")
    } else if item_type.contains("PlayerMeleeWeaponRandomMod") {
        Some("Melee")
    } else if item_type.contains("LotusArchgunRandomMod") {
        Some("Archgun")
    } else if item_type.contains("LotusModularPistolRandomMod") {
        Some("Kitgun")
    } else {
        None
    }
}

fn prettify(value: &str) -> String {
    let last = value.rsplit('/').next().unwrap_or(value);
    let mut words = String::new();
    for (index, ch) in last.chars().enumerate() {
        if index > 0 && ch.is_uppercase() {
            words.push(' ');
        }
        words.push(ch);
    }
    words
}

#[derive(Deserialize, Clone)]
struct BaseEntry {
    value: f64,
    #[serde(default)]
    prefix: String,
    #[serde(default)]
    suffix: String,
}

fn base_tables() -> HashMap<String, HashMap<String, BaseEntry>> {
    serde_json::from_str(include_str!("../data/riven-base.json")).unwrap_or_default()
}

// The engine stores the random roll as a normalized 30-bit fraction. The
// displayed number also depends on the stat's base coefficient for the weapon
// class, the riven's rank, how many bonuses/curses it rolled, and the
// disposition, which is a flat 1.2 since 44.1.3.
const DISPOSITION: f64 = 1.2;
const BUFF_ATTEN: [f64; 6] = [0.0, 1.0, 0.660_000_03, 0.5, 0.400_000_01, 0.349_999_99];
const CURSE_ATTEN: [f64; 6] = [0.0, 1.0, 0.330_000_01, 0.5, 1.25, 1.5];

fn riven_roll(raw: u64) -> f64 {
    let roll = raw as f64 / 1_073_741_823.0;
    if (0.0..=1.0).contains(&roll) {
        roll
    } else {
        0.0
    }
}

fn is_faction(tag: &str) -> bool {
    tag.starts_with("WeaponFactionDamage") || tag.starts_with("WeaponMeleeFactionDamage")
}

fn is_flat(tag: &str) -> bool {
    matches!(
        tag,
        "WeaponMeleeComboInitialBonusMod" | "ComboDurationMod" | "WeaponMeleeRangeIncMod"
    )
}

// Ammo efficiency and combo chance display inverted: a bigger roll is a
// stronger stat, so the game prints -100v%.
fn is_inverted(tag: &str) -> bool {
    matches!(
        tag,
        "WeaponAmmoEfficiency" | "WeaponMeleeComboPointsOnHitMod"
    )
}

fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn computed_value(
    tag: &str,
    base: f64,
    buffs: usize,
    curses: usize,
    lvl: u64,
    raw: u64,
    positive: bool,
) -> f64 {
    let roll = 0.9 + 0.2 * riven_roll(raw);
    let rank = lvl as f64 + 1.0;
    let value = if positive {
        base * 1.5
            * DISPOSITION
            * 10.0
            * 1.25f64.powi(curses as i32)
            * roll
            * BUFF_ATTEN[buffs.min(5)]
            * rank
    } else {
        -base
            * 1.5
            * DISPOSITION
            * 10.0
            * roll
            * CURSE_ATTEN[buffs.min(5)]
            * BUFF_ATTEN[curses.min(5)]
            * rank
    };
    if is_faction(tag) {
        round2(value)
    } else if is_flat(tag) {
        round1(value)
    } else if is_inverted(tag) {
        round1(-value * 100.0)
    } else {
        round1(value * 100.0)
    }
}

fn stat_unit(unit: Option<&str>) -> &'static str {
    match unit {
        Some("percent") => "%",
        Some("multiply") => "x",
        Some("seconds") => "s",
        _ => "",
    }
}

fn words(
    tag: &str,
    bases: &HashMap<String, BaseEntry>,
    attributes_by_tag: &HashMap<String, Attribute>,
) -> (String, String) {
    if let Some(base) = bases.get(tag) {
        if !base.prefix.is_empty() || !base.suffix.is_empty() {
            return (base.prefix.to_lowercase(), base.suffix.to_lowercase());
        }
    }
    attributes_by_tag
        .get(tag)
        .map(|attribute| {
            (
                attribute.prefix.to_lowercase(),
                attribute.suffix.to_lowercase(),
            )
        })
        .unwrap_or_default()
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

// The game composes the name from the two or three positive rolls: the first
// contributes its prefix (title-cased), the last its suffix, and a middle one
// adds "-prefix". Curses do not take part.
fn build_name(
    buffs: &[Buff],
    bases: &HashMap<String, BaseEntry>,
    attributes_by_tag: &HashMap<String, Attribute>,
) -> String {
    let mut roll_by_tag: HashMap<&str, u64> = HashMap::new();
    for buff in buffs {
        let roll = roll_by_tag.entry(buff.tag.as_str()).or_insert(0);
        *roll = (*roll).max(buff.value);
    }
    let mut sorted: Vec<&Buff> = buffs
        .iter()
        .filter(|buff| bases.contains_key(&buff.tag))
        .collect();
    sorted.sort_by(|left, right| {
        let by_roll = roll_by_tag[right.tag.as_str()].cmp(&roll_by_tag[left.tag.as_str()]);
        if by_roll != std::cmp::Ordering::Equal {
            return by_roll;
        }
        let left_value = bases.get(&left.tag).map_or(0.0, |base| base.value);
        let right_value = bases.get(&right.tag).map_or(0.0, |base| base.value);
        left_value
            .partial_cmp(&right_value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut name = String::new();
    for (index, buff) in sorted.iter().enumerate() {
        let (prefix, suffix) = words(&buff.tag, bases, attributes_by_tag);
        if index + 1 == sorted.len() {
            name.push_str(&suffix);
        } else if index == 0 {
            name.push_str(&title_case(&prefix));
        } else {
            name.push('-');
            name.push_str(&prefix);
        }
    }
    name
}

fn stats_for(
    buffs: &[Buff],
    curses: &[Buff],
    lvl: u64,
    bases: &HashMap<String, BaseEntry>,
    attributes_by_tag: &HashMap<String, Attribute>,
) -> Vec<RivenStatDto> {
    let mut stats = Vec::with_capacity(buffs.len() + curses.len());
    for (group, positive) in [(buffs, true), (curses, false)] {
        for buff in group {
            let Some(base) = bases.get(&buff.tag) else {
                continue;
            };
            let attribute = attributes_by_tag.get(&buff.tag);
            let value = computed_value(
                &buff.tag,
                base.value,
                buffs.len(),
                curses.len(),
                lvl,
                buff.value,
                positive,
            );
            stats.push(RivenStatDto {
                name: attribute
                    .and_then(|attribute| attribute.i18n.name())
                    .map(str::to_string)
                    .unwrap_or_else(|| prettify(&buff.tag)),
                slug: attribute
                    .map(|attribute| attribute.slug.clone())
                    .unwrap_or_default(),
                value: value.abs(),
                unit: stat_unit(attribute.and_then(|attribute| attribute.unit.as_deref()))
                    .to_string(),
                positive: value >= 0.0,
            });
        }
    }
    stats
}

pub fn inventory() -> Result<Vec<RivenDto>, String> {
    let entries = inventory::raw_upgrades()?;
    let weapons = weapons_by_game_ref();
    let attributes = attributes();
    let attributes_by_tag: HashMap<String, Attribute> = attributes
        .iter()
        .map(|attribute| (attribute.game_ref.clone(), attribute.clone()))
        .collect();
    let all_bases = base_tables();

    let mut rivens = Vec::new();
    for entry in entries {
        let item_type = entry
            .get("ItemType")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !item_type.contains("Randomized") {
            continue;
        }
        let Some(fingerprint) = fingerprint(&entry) else {
            continue;
        };
        let id = entry
            .get("ItemId")
            .and_then(|value| value.get("$oid"))
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some(id) = id else { continue };

        let weapon = fingerprint
            .compat
            .as_deref()
            .and_then(|path| weapons.get(path));
        let veiled = fingerprint.compat.is_none();
        let challenge = fingerprint
            .challenge
            .as_ref()
            .and_then(|value| value.get("Type"))
            .and_then(Value::as_str)
            .map(prettify);
        let display_class = weapon
            .and_then(|weapon| weapon.riven_type.clone())
            .or_else(|| weapon_class(item_type).map(str::to_string));
        let weapon_name = weapon.and_then(|weapon| weapon.i18n.name().map(str::to_string));
        let class = item_type
            .rsplit('/')
            .next()
            .unwrap_or(item_type)
            .to_string();
        let bases = all_bases.get(&class).cloned().unwrap_or_default();
        let generated = build_name(&fingerprint.buffs, &bases, &attributes_by_tag);
        let name = if veiled {
            format!(
                "Veiled {} riven",
                display_class.as_deref().unwrap_or("weapon")
            )
        } else if generated.is_empty() {
            weapon_name
                .as_ref()
                .map(|weapon| format!("{weapon} riven"))
                .unwrap_or_else(|| "Riven mod".to_string())
        } else {
            generated
        };

        rivens.push(RivenDto {
            id,
            weapon: weapon_name,
            weapon_slug: weapon.map(|weapon| weapon.slug.clone()),
            weapon_type: display_class,
            weapon_thumb: weapon.and_then(|weapon| weapon.i18n.thumb().map(str::to_string)),
            polarity: polarity(fingerprint.pol.as_deref()),
            rank: fingerprint.lvl.unwrap_or(0),
            mastery: fingerprint.lvl_req,
            rerolls: fingerprint.rerolls.unwrap_or(0),
            veiled,
            challenge,
            stats: if veiled {
                Vec::new()
            } else {
                stats_for(
                    &fingerprint.buffs,
                    &fingerprint.curses,
                    fingerprint.lvl.unwrap_or(0),
                    &bases,
                    &attributes_by_tag,
                )
            },
            name,
        });
    }
    rivens.sort_by(|left, right| {
        right
            .veiled
            .cmp(&left.veiled)
            .then_with(|| left.name.cmp(&right.name))
    });
    Ok(rivens)
}

fn account_slug() -> Result<String, String> {
    let token = crate::account::load_token()
        .ok_or_else(|| "Connect a warframe.market account in Settings.".to_string())?;
    let response = fetch::request(
        "GET",
        "https://api.warframe.market/v2/me",
        Some(&token),
        None,
    )?;
    if !(200..300).contains(&response.status) {
        return Err("warframe.market did not accept the session.".to_string());
    }
    let doc: Value =
        serde_json::from_str(&response.body).map_err(|error| format!("me json: {error}"))?;
    let slug = doc
        .get("data")
        .and_then(|data| data.get("slug"))
        .and_then(Value::as_str)
        .ok_or_else(|| "warframe.market returned no profile slug.".to_string())?;
    if !valid_profile_slug(slug) {
        return Err("warframe.market returned an invalid profile slug.".to_string());
    }
    Ok(slug.to_string())
}

// The slug is interpolated into a URL path, so it is one safe component only.
fn valid_profile_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= 100
        && slug != ".."
        && !slug.contains("..")
        && slug
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

// The profile auctions route is public; hidden listings are the only thing it
// omits, and those need the website's CSRF token, which the API never exposes.
pub fn listings() -> Result<Vec<RivenListingDto>, String> {
    let slug = account_slug()?;
    let url = format!("https://api.warframe.market/v1/profile/{slug}/auctions");
    let response = fetch::request("GET", &url, None, None)?;
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "warframe.market listings: HTTP {}",
            response.status
        ));
    }
    let doc: ProfileDoc =
        serde_json::from_str(&response.body).map_err(|error| format!("listings json: {error}"))?;
    let attributes = attributes();
    let by_slug: HashMap<String, Attribute> = attributes
        .into_iter()
        .map(|attribute| (attribute.slug.clone(), attribute))
        .collect();
    let weapons = weapons_by_game_ref();
    let weapons_by_slug: HashMap<&str, &Weapon> = weapons
        .values()
        .map(|weapon| (weapon.slug.as_str(), weapon))
        .collect();

    let mut listings = Vec::new();
    for auction in doc.payload.auctions {
        if auction.closed {
            continue;
        }
        let weapon = weapons_by_slug.get(auction.item.weapon_url_name.as_str());
        let weapon_name = weapon.and_then(|weapon| weapon.i18n.name().map(str::to_string));
        let stats = auction
            .item
            .attributes
            .into_iter()
            .map(|stat| {
                let attribute = by_slug.get(&stat.url_name);
                RivenListingStatDto {
                    name: attribute
                        .and_then(|attribute| attribute.i18n.name())
                        .map(str::to_string)
                        .unwrap_or_else(|| prettify(&stat.url_name)),
                    url_name: stat.url_name,
                    value: (stat.value * 10.0).round() / 10.0,
                    unit: stat_unit(attribute.and_then(|a| a.unit.as_deref())).to_string(),
                    positive: stat.positive,
                }
            })
            .collect();
        listings.push(RivenListingDto {
            id: auction.id,
            direct: auction.is_direct_sell,
            starting_price: auction.starting_price,
            buyout_price: auction.buyout_price,
            top_bid: auction.top_bid,
            created: auction.created,
            visible: auction.visible,
            note: auction.note,
            name: auction.item.name,
            weapon: weapon_name,
            weapon_slug: auction.item.weapon_url_name,
            weapon_thumb: weapon.and_then(|weapon| weapon.i18n.thumb().map(str::to_string)),
            rank: auction.item.mod_rank,
            rerolls: auction.item.re_rolls.unwrap_or(0),
            mastery: auction.item.mastery_level,
            polarity: auction.item.polarity,
            stats,
        });
    }
    listings.sort_by(|left, right| right.created.cmp(&left.created));
    Ok(listings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_slug_is_a_single_url_safe_component() {
        assert!(valid_profile_slug("tenno-123"));
        assert!(valid_profile_slug("Tenno.Name_1"));
        assert!(!valid_profile_slug(""));
        assert!(!valid_profile_slug(".."));
        assert!(!valid_profile_slug("../admin"));
        assert!(!valid_profile_slug("name?x=1"));
        assert!(!valid_profile_slug("name/auctions"));
        assert!(!valid_profile_slug("with space"));
    }

    #[test]
    fn computes_display_values_with_rank_and_curse() {
        // Stug riven at rank 8 with two positives and one curse.
        assert_eq!(
            computed_value("WeaponFireRateMod", 0.0083, 2, 1, 8, 338_071_530, true),
            106.8
        );
        assert_eq!(
            computed_value("WeaponDamageAmountMod", 0.0244, 2, 1, 8, 35_815_502, true),
            295.7
        );
        assert_eq!(
            computed_value("WeaponAmmoMaxMod", 0.01, 2, 1, 8, 882_502_854, false),
            -56.9
        );
    }

    #[test]
    fn generates_a_riven_name() {
        let bases: HashMap<String, BaseEntry> = [
            (
                "WeaponFireRateMod".to_string(),
                BaseEntry {
                    value: 0.0083,
                    prefix: "croni".into(),
                    suffix: "dra".into(),
                },
            ),
            (
                "WeaponDamageAmountMod".to_string(),
                BaseEntry {
                    value: 0.0244,
                    prefix: "visi".into(),
                    suffix: "ata".into(),
                },
            ),
        ]
        .into_iter()
        .collect();
        let buffs = vec![
            Buff {
                tag: "WeaponDamageAmountMod".into(),
                value: 35_815_502,
            },
            Buff {
                tag: "WeaponFireRateMod".into(),
                value: 338_071_530,
            },
        ];
        assert_eq!(build_name(&buffs, &bases, &HashMap::new()), "Croniata");
    }

    #[test]
    fn maps_polarities() {
        assert_eq!(polarity(Some("AP_ATTACK")), "Madurai");
        assert_eq!(polarity(Some("AP_DEFENSE")), "Vazarin");
        assert_eq!(polarity(Some("AP_TACTIC")), "Naramon");
    }

    #[test]
    fn reads_the_market_weapon_thumb() {
        let weapon: Weapon = serde_json::from_str(
            r#"{"slug":"battacor","gameRef":"/Lotus/Weapons/Corpus/LongGuns/CrpSentExperimentRifle/CrpSentExperimentRifle","i18n":{"en":{"name":"Battacor","thumb":"items/images/en/thumbs/battacor.128x128.webp"}}}"#,
        )
        .unwrap();
        assert_eq!(
            weapon.i18n.thumb(),
            Some("items/images/en/thumbs/battacor.128x128.webp")
        );

        let without: Weapon =
            serde_json::from_str(r#"{"slug":"x","gameRef":"/x","i18n":{"en":{"name":"X"}}}"#)
                .unwrap();
        assert_eq!(without.i18n.thumb(), None);
    }

    #[test]
    fn parses_a_riven_fingerprint() {
        let entry: Value = serde_json::from_str(
            r#"{"ItemType":"/Lotus/Upgrades/Mods/Randomized/LotusRifleRandomModRare","UpgradeFingerprint":"{\"compat\":\"/Lotus/Weapons/Tenno/Rifle/Rifle\",\"lvl\":8,\"lvlReq\":14,\"rerolls\":68,\"pol\":\"AP_TACTIC\",\"buffs\":[{\"Tag\":\"WeaponCritChanceMod\",\"Value\":824484087}],\"curses\":[{\"Tag\":\"WeaponZoomFovMod\",\"Value\":583614220}]}"}"#,
        )
        .unwrap();
        let fingerprint = fingerprint(&entry).unwrap();
        assert_eq!(fingerprint.lvl, Some(8));
        assert_eq!(fingerprint.lvl_req, Some(14));
        assert_eq!(fingerprint.buffs[0].tag, "WeaponCritChanceMod");
        assert_eq!(fingerprint.curses.len(), 1);
        assert_eq!(
            prettify("/Lotus/Types/Challenges/HighPerfectDefense"),
            "High Perfect Defense"
        );
    }
}
