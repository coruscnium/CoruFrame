use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use coruframe_core::fetch;

// The bare /pc route started returning 404; the language query selects the
// localized worldstate document and is cached the same way.
const WORLDSTATE_URL: &str = "https://api.warframestat.us/pc?language=en";
const WORLDSTATE_MAX_AGE: Duration = Duration::from_secs(60);

// The community document has been frozen for hours at a time. Fissures cannot
// be projected like cycles, so a stale document falls back to DE's own export.
const OFFICIAL_URL: &str = "https://api.warframe.com/cdn/worldState.php";
const SOL_NODES_URL: &str =
    "https://raw.githubusercontent.com/wfcd/warframe-worldstate-data/master/data/solNodes.json";

#[derive(Debug, Deserialize)]
struct Doc {
    #[serde(default)]
    timestamp: String,
    #[serde(rename = "earthCycle")]
    earth: Cycle,
    #[serde(rename = "cetusCycle")]
    cetus: Cycle,
    #[serde(rename = "vallisCycle")]
    vallis: Cycle,
    #[serde(rename = "cambionCycle")]
    cambion: Cycle,
    #[serde(default)]
    fissures: Vec<Fissure>,
    #[serde(default)]
    alerts: Vec<Alert>,
    #[serde(default, rename = "duviriCycle")]
    duviri: Option<Duviri>,
}

#[derive(Debug, Deserialize)]
struct Alert {
    #[serde(default)]
    expiry: String,
    #[serde(default)]
    mission: AlertMission,
}

#[derive(Debug, Default, Deserialize)]
struct AlertMission {
    #[serde(default)]
    node: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    reward: AlertReward,
}

#[derive(Debug, Default, Deserialize)]
struct AlertReward {
    #[serde(default)]
    credits: u64,
    #[serde(default)]
    items: Vec<AlertItem>,
    #[serde(default, rename = "countedItems")]
    counted: Vec<AlertItem>,
}

#[derive(Debug, Default, Deserialize)]
struct AlertItem {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    count: u64,
}

#[derive(Debug, Deserialize)]
struct Duviri {
    #[serde(default)]
    choices: Vec<DuviriChoice>,
}

#[derive(Debug, Deserialize)]
struct DuviriChoice {
    #[serde(default)]
    category: String,
    #[serde(default)]
    choices: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct OfficialDoc {
    #[serde(default, rename = "ActiveMissions")]
    active: Vec<OfficialMission>,
    #[serde(default, rename = "VoidStorms")]
    storms: Vec<OfficialMission>,
}

#[derive(Debug, Deserialize)]
struct OfficialMission {
    #[serde(default, rename = "Node")]
    node: String,
    #[serde(default, rename = "Modifier")]
    modifier: String,
    #[serde(default, rename = "ActiveMissionTier")]
    tier: String,
    #[serde(default, rename = "Hard")]
    hard: bool,
    #[serde(default, rename = "Expiry")]
    expiry: Option<OfficialDate>,
}

#[derive(Debug, Deserialize)]
struct OfficialDate {
    #[serde(rename = "$date")]
    date: OfficialMillis,
}

#[derive(Debug, Deserialize)]
struct OfficialMillis {
    #[serde(rename = "$numberLong")]
    millis: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct SolNode {
    #[serde(default)]
    value: String,
    #[serde(default, rename = "type")]
    kind: String,
}

#[derive(Debug, Deserialize)]
struct Cycle {
    #[serde(default)]
    activation: String,
    #[serde(default)]
    expiry: String,
    #[serde(default)]
    state: String,
    #[serde(default, rename = "timeLeft")]
    time_left: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Fissure {
    tier: String,
    #[serde(rename = "missionType")]
    mission_type: String,
    node: String,
    #[serde(default)]
    expiry: String,
    #[serde(default, rename = "isHard")]
    is_hard: bool,
    #[serde(default, rename = "isStorm")]
    is_storm: bool,
}

#[derive(Debug, Serialize)]
pub struct CycleDto {
    pub id: String,
    pub name: String,
    pub state: String,
    pub activation: String,
    pub expiry: String,
    pub time_left: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FissureDto {
    pub tier: String,
    pub mission_type: String,
    pub node: String,
    pub expiry: String,
    pub is_hard: bool,
    pub is_storm: bool,
}

#[derive(Debug, Default, Serialize)]
pub struct CircuitDto {
    pub normal: Vec<String>,
    pub hard: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct AlertDto {
    pub mission_type: String,
    pub node: String,
    pub reward: String,
    pub expiry: String,
}

fn alert_reward(reward: &AlertReward) -> String {
    let mut parts = Vec::new();
    for item in reward.items.iter().chain(reward.counted.iter()) {
        if item.kind.is_empty() {
            continue;
        }
        if item.count > 1 {
            parts.push(format!("{} × {}", item.count, item.kind));
        } else {
            parts.push(item.kind.clone());
        }
    }
    if reward.credits > 0 {
        parts.push(format!("{} credits", reward.credits));
    }
    parts.join(" + ")
}

#[derive(Debug, Serialize)]
pub struct WorldstateDto {
    pub cycles: Vec<CycleDto>,
    pub fissures: Vec<FissureDto>,
    pub alerts: Vec<AlertDto>,
    pub circuit: CircuitDto,
    // The community document is behind; the panel keeps its last rows visible
    // instead of letting them expire off the list.
    pub stale: bool,
    pub updated_at: u64,
    pub upstream: String,
}

fn cycle(id: &str, name: &str, cycle: Cycle) -> CycleDto {
    CycleDto {
        id: id.to_string(),
        name: name.to_string(),
        state: cycle.state,
        activation: cycle.activation,
        expiry: cycle.expiry,
        time_left: cycle.time_left,
    }
}

fn millis_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

// RFC 3339 in UTC. The upstream timestamps are compared as strings, so this
// must stay fixed-width and lexicographically ordered.
fn iso_utc(millis: i64) -> String {
    let seconds = millis.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

// Howard Hinnant's days-from-civil, inverted, without pulling in a date crate.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (if month <= 2 { year + 1 } else { year }, month, day)
}

fn upstream_is_stale(timestamp: &str) -> bool {
    if timestamp.is_empty() {
        return true;
    }
    let cutoff = iso_utc(millis_now() - 10 * 60 * 1000);
    timestamp < cutoff.as_str()
}

fn tier_of(modifier: &str) -> &'static str {
    match modifier {
        "VoidT1" => "Lith",
        "VoidT2" => "Meso",
        "VoidT3" => "Neo",
        "VoidT4" => "Axi",
        "VoidT5" => "Requiem",
        "VoidT6" => "Omnia",
        _ => "",
    }
}

fn millis_of(value: &serde_json::Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn official_fissures_from(doc_json: &str, nodes_json: &str) -> Result<Vec<FissureDto>, String> {
    let doc: OfficialDoc =
        serde_json::from_str(doc_json).map_err(|error| format!("official json: {error}"))?;
    let nodes: HashMap<String, SolNode> =
        serde_json::from_str(nodes_json).map_err(|error| format!("node json: {error}"))?;
    let mut rows = Vec::new();
    for (mission, storm) in doc
        .active
        .into_iter()
        .map(|mission| (mission, false))
        .chain(doc.storms.into_iter().map(|mission| (mission, true)))
    {
        let tier = tier_of(if mission.modifier.is_empty() {
            &mission.tier
        } else {
            &mission.modifier
        });
        if tier.is_empty() {
            continue;
        }
        let node = nodes.get(&mission.node);
        let expiry = mission
            .expiry
            .as_ref()
            .and_then(|date| millis_of(&date.date.millis))
            .map(iso_utc)
            .unwrap_or_default();
        rows.push(FissureDto {
            tier: tier.to_string(),
            mission_type: node
                .map(|entry| entry.kind.clone())
                .filter(|kind| !kind.is_empty())
                .unwrap_or_else(|| "Mission".to_string()),
            node: node
                .map(|entry| entry.value.clone())
                .unwrap_or_else(|| mission.node.clone()),
            expiry,
            is_hard: mission.hard,
            is_storm: storm,
        });
    }
    Ok(rows)
}

fn official_fissures() -> Result<Vec<FissureDto>, String> {
    let doc = fetch::fetch(OFFICIAL_URL, "worldstate-official.json", WORLDSTATE_MAX_AGE)?;
    let nodes = fetch::fetch(
        SOL_NODES_URL,
        "solnodes.json",
        Duration::from_secs(7 * 24 * 3600),
    )?;
    official_fissures_from(&doc, &nodes)
}

pub fn load(force: bool) -> Result<WorldstateDto, String> {
    // A cycle that just expired must not be answered from the 60-second cache:
    // that cache would still describe the old state.
    let max_age = if force {
        Duration::ZERO
    } else {
        WORLDSTATE_MAX_AGE
    };
    let json = fetch::fetch(WORLDSTATE_URL, "worldstate-pc.json", max_age)?;
    let doc: Doc =
        serde_json::from_str(&json).map_err(|error| format!("worldstate json: {error}"))?;
    let mut fissures: Vec<FissureDto> = doc
        .fissures
        .into_iter()
        .map(|fissure| FissureDto {
            tier: fissure.tier,
            mission_type: fissure.mission_type,
            node: fissure.node,
            expiry: fissure.expiry,
            is_hard: fissure.is_hard,
            is_storm: fissure.is_storm,
        })
        .collect();
    // Cycles can be projected from a frozen document, fissure windows cannot:
    // read the live official export whenever the community feed is behind.
    let stale = upstream_is_stale(&doc.timestamp);
    if stale {
        if let Ok(official) = official_fissures() {
            if !official.is_empty() {
                fissures = official;
            }
        }
    }
    let alerts = doc
        .alerts
        .into_iter()
        .map(|alert| {
            let AlertMission { node, kind, reward } = alert.mission;
            AlertDto {
                mission_type: kind,
                node,
                reward: alert_reward(&reward),
                expiry: alert.expiry,
            }
        })
        .collect();
    let circuit = doc
        .duviri
        .as_ref()
        .map(|duviri| CircuitDto {
            normal: duviri
                .choices
                .iter()
                .find(|choice| choice.category == "normal")
                .map(|choice| choice.choices.clone())
                .unwrap_or_default(),
            hard: duviri
                .choices
                .iter()
                .find(|choice| choice.category == "hard")
                .map(|choice| choice.choices.clone())
                .unwrap_or_default(),
        })
        .unwrap_or_default();
    Ok(WorldstateDto {
        cycles: vec![
            cycle("earth", "Earth", doc.earth),
            cycle("cetus", "Cetus", doc.cetus),
            cycle("vallis", "Vallis", doc.vallis),
            cycle("cambion", "Cambion", doc.cambion),
        ],
        fissures,
        alerts,
        circuit,
        stale,
        updated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0),
        upstream: doc.timestamp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = r#"{
      "earthCycle":{"activation":"2026-10-24T00:00:00.263Z","expiry":"2026-10-24T04:00:00.263Z","state":"day","timeLeft":"3h 44m 0s"},
      "cetusCycle":{"activation":"2026-10-23T23:52:00.000Z","expiry":"2026-10-24T00:42:00.000Z","state":"night","timeLeft":"26m 0s"},
      "vallisCycle":{"activation":"2026-10-24T00:13:00.000Z","expiry":"2026-10-24T00:20:08.000Z","state":"warm"},
      "cambionCycle":{"activation":"2026-10-23T23:52:00.000Z","expiry":"2026-10-24T00:42:00.000Z","state":"vome","timeLeft":"26m 0s"},
      "fissures":[{"tier":"Neo","missionType":"Spy","node":"Nereid (Neptune)","expiry":"2026-10-24T00:43:56.081Z"}],
      "alerts":[{"expiry":"2026-10-24T01:00:00.000Z","mission":{
        "node":"Nimus (Eris)","type":"Hive",
        "reward":{"credits":17000,"items":[],"countedItems":[{"type":"Road To Tau Event Resource","count":50}]}
      }}],
      "duviriCycle":{"state":"envy","choices":[
        {"category":"normal","choices":["Hydroid","Mirage","Limbo"]},
        {"category":"hard","choices":["Dera","Cestra","Okina"]}
      ]}
    }"#;

    #[test]
    fn parses_cycles_and_fissures() {
        let doc: Doc = serde_json::from_str(DOC).unwrap();
        assert_eq!(doc.cetus.state, "night");
        assert_eq!(doc.vallis.time_left, None);
        assert_eq!(doc.fissures[0].tier, "Neo");
        assert_eq!(doc.fissures[0].mission_type, "Spy");
        let duviri = doc.duviri.unwrap();
        assert_eq!(duviri.choices[0].choices, ["Hydroid", "Mirage", "Limbo"]);
        assert_eq!(duviri.choices[1].choices, ["Dera", "Cestra", "Okina"]);
        let alert = &doc.alerts[0];
        assert_eq!(alert.mission.kind, "Hive");
        assert_eq!(
            alert_reward(&alert.mission.reward),
            "50 × Road To Tau Event Resource + 17000 credits"
        );
    }

    #[test]
    fn formats_utc_timestamps() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_792_914_864_000), "2026-10-25T07:54:24Z");
        assert_eq!(
            iso_utc(1_792_914_864_000 + 86_400_000),
            "2026-10-26T07:54:24Z"
        );
    }

    #[test]
    fn detects_a_stale_upstream() {
        assert!(upstream_is_stale("2000-01-01T00:00:00.000Z"));
        assert!(!upstream_is_stale("2999-01-01T00:00:00.000Z"));
    }

    const OFFICIAL: &str = r#"{
      "ActiveMissions":[
        {"Node":"SolNode153","Modifier":"VoidT4","Hard":true,
         "Expiry":{"$date":{"$numberLong":"1792914893199"}}},
        {"Node":"SolNode10","Modifier":"VoidT1","Hard":false,
         "Expiry":{"$date":{"$numberLong":"1792914600000"}}}
      ],
      "VoidStorms":[
        {"Node":"CrewBattleNode522","ActiveMissionTier":"VoidT2",
         "Expiry":{"$date":{"$numberLong":"1792914603351"}}}
      ]
    }"#;

    const NODES: &str = r#"{
      "SolNode153":{"value":"Brugia (Eris)","enemy":"Infested","type":"Rescue"},
      "SolNode10":{"value":"E Prime (Earth)","enemy":"Grineer","type":"Extermination"},
      "CrewBattleNode522":{"value":"Bendar Cluster (Earth)","enemy":"Grineer","type":"Skirmish"}
    }"#;

    #[test]
    fn maps_official_fissures_through_the_node_table() {
        let rows = official_fissures_from(OFFICIAL, NODES).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].tier, "Axi");
        assert_eq!(rows[0].mission_type, "Rescue");
        assert_eq!(rows[0].node, "Brugia (Eris)");
        assert_eq!(rows[0].expiry, "2026-10-25T07:54:53Z");
        assert!(rows[0].is_hard);
        assert!(!rows[0].is_storm);
        assert_eq!(rows[1].tier, "Lith");
        assert!(!rows[1].is_hard);
        assert_eq!(rows[2].tier, "Meso");
        assert!(rows[2].is_storm);
        assert_eq!(rows[2].mission_type, "Skirmish");
    }
}
