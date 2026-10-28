use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Loadout {
    pub suits: Vec<String>,
    pub weapons: Vec<String>,
    pub mods: Vec<String>,
    pub arcanes: Vec<String>,
    pub gear: Vec<String>,
    // The context dump is wider than a loadout; keep the rest so nothing is silently dropped.
    pub other: Vec<String>,
}

impl Loadout {
    fn is_empty(&self) -> bool {
        self.suits.is_empty()
            && self.weapons.is_empty()
            && self.mods.is_empty()
            && self.arcanes.is_empty()
            && self.gear.is_empty()
    }

    fn add(&mut self, path: &str) {
        let slot = if path.ends_with("AugmentCard") || path.contains("/Lotus/Upgrades/Mods/") {
            &mut self.mods
        } else if path.contains("/Lotus/Upgrades/CosmeticEnhancers/") {
            &mut self.arcanes
        } else if path.contains("/Lotus/Types/Restoratives/") {
            &mut self.gear
        } else if path.starts_with("/Lotus/Powersuits/")
            && !path.contains("/PowersuitAbilities/")
            && !path.ends_with("Ability")
        {
            &mut self.suits
        } else if path.contains("/Lotus/Weapons/") {
            &mut self.weapons
        } else {
            &mut self.other
        };
        slot.push(path.to_string());
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Login {
        line: u64,
        ts_ms: u64,
        user: String,
    },
    Loadout {
        line: u64,
        ts_ms: u64,
        loadout: Loadout,
    },
    MissionEnd {
        line: u64,
        ts_ms: u64,
        success: bool,
        reward_tiers: Vec<u32>,
        credit_bonus: u64,
    },
    RelicExtraction {
        line: u64,
        ts_ms: u64,
    },
    // The log names the relic you equipped and extraction start, but never the reward item.
    RelicEquipped {
        line: u64,
        ts_ms: u64,
        relic: String,
        state: String,
    },
    RelicReward {
        line: u64,
        ts_ms: u64,
        player_id: String,
        game_ref: String,
    },
    // `AddSquadMember: <name>, mm=..., squadCount=N` is the squad size, an
    // upper bound on the reward cards: a player whose relic fails to crack
    // stays in the squad but gets no card.
    SquadSize {
        line: u64,
        ts_ms: u64,
        count: u32,
    },
}

#[derive(Default)]
struct Mission {
    ts_ms: u64,
    success: bool,
    reward_tiers: Vec<u32>,
    credit_bonus: Option<u64>,
}

pub struct Parser {
    line: u64,
    in_context: bool,
    context_ts: u64,
    context: Loadout,
    mission: Option<Mission>,
}

impl Default for Parser {
    fn default() -> Self {
        Self::new()
    }
}

impl Parser {
    pub fn new() -> Self {
        Self {
            line: 0,
            in_context: false,
            context_ts: 0,
            context: Loadout::default(),
            mission: None,
        }
    }

    // A live follower starts at the end of the file; this keeps event line numbers absolute.
    pub fn starting_after(line: u64) -> Self {
        Self {
            line,
            ..Self::new()
        }
    }

    pub fn push(&mut self, line: &str) -> Vec<Event> {
        self.line += 1;
        let Some((ts_ms, subsystem, text)) = split(line) else {
            return Vec::new();
        };
        let mut events = Vec::new();

        if self.in_context && subsystem == "Game" {
            if text.starts_with("/Lotus/") {
                self.context.add(text);
                return events;
            }
            self.close_context(&mut events);
        }

        if subsystem == "Game" && text.ends_with("Server::LoadClient: Context objects:") {
            self.in_context = true;
            self.context_ts = ts_ms;
            self.context = Loadout::default();
        } else if subsystem == "Sys" && text.starts_with("Logged in ") {
            events.push(Event::Login {
                line: self.line,
                ts_ms,
                user: text["Logged in ".len()..].to_string(),
            });
        } else if let Some(rest) = text.strip_prefix("EndOfMatch.lua: ") {
            self.mission_line(ts_ms, rest, &mut events);
        } else if text.contains("OpenVoidProjectionRewardScreen") {
            // Extraction started; the reward cards follow after an animation.
            events.push(Event::RelicExtraction {
                line: self.line,
                ts_ms,
            });
        } else if let Some((_, description)) =
            text.split_once("Dialog::CreateOkCancel(description=")
        {
            if let Some((relic, state)) = relic_equip(description) {
                events.push(Event::RelicEquipped {
                    line: self.line,
                    ts_ms,
                    relic,
                    state,
                });
            }
        } else if let Some(rest) = text.strip_prefix("VoidProjections: ") {
            if let Some((player_id, game_ref)) = rest.split_once(" gets reward ") {
                if game_ref.starts_with("/Lotus/") {
                    events.push(Event::RelicReward {
                        line: self.line,
                        ts_ms,
                        player_id: player_id.to_string(),
                        game_ref: game_ref.to_string(),
                    });
                }
            }
        } else if text.starts_with("AddSquadMember: ") {
            if let Some(count) = text
                .rsplit_once("squadCount=")
                .and_then(|(_, value)| value.trim().parse::<u32>().ok())
                .filter(|count| (1..=4).contains(count))
            {
                events.push(Event::SquadSize {
                    line: self.line,
                    ts_ms,
                    count,
                });
            }
        }

        events
    }

    pub fn finish(&mut self) -> Vec<Event> {
        let mut events = Vec::new();
        self.close_context(&mut events);
        if let Some(mission) = self.mission.take() {
            if mission.success {
                events.push(Event::MissionEnd {
                    line: self.line,
                    ts_ms: mission.ts_ms,
                    success: true,
                    reward_tiers: mission.reward_tiers,
                    credit_bonus: mission.credit_bonus.unwrap_or(0),
                });
            }
        }
        events
    }

    fn close_context(&mut self, events: &mut Vec<Event>) {
        self.in_context = false;
        if !self.context.is_empty() {
            events.push(Event::Loadout {
                line: self.line,
                ts_ms: self.context_ts,
                loadout: std::mem::take(&mut self.context),
            });
        }
    }

    fn mission_line(&mut self, ts_ms: u64, text: &str, events: &mut Vec<Event>) {
        if text == "Initialize" {
            if let Some(mission) = self.mission.take() {
                if mission.success {
                    events.push(Event::MissionEnd {
                        line: self.line,
                        ts_ms: mission.ts_ms,
                        success: true,
                        reward_tiers: mission.reward_tiers,
                        credit_bonus: mission.credit_bonus.unwrap_or(0),
                    });
                }
            }
            self.mission = Some(Mission {
                ts_ms,
                ..Mission::default()
            });
            return;
        }
        let Some(mission) = self.mission.as_mut() else {
            return;
        };
        if text == "Mission Succeeded" {
            mission.success = true;
        } else if let Some(tier) = text
            .rsplit_once("Gave reward tier ")
            .and_then(|(_, n)| n.parse().ok())
        {
            mission.reward_tiers.push(tier);
        } else if let Some(bonus) = text.strip_prefix("GetMissionCompleteCreditBonus: ") {
            mission.credit_bonus = Some(bonus.parse().unwrap_or(0));
            let mission = self.mission.take().expect("mission checked above");
            events.push(Event::MissionEnd {
                line: self.line,
                ts_ms,
                success: mission.success,
                reward_tiers: mission.reward_tiers,
                credit_bonus: mission.credit_bonus.unwrap_or(0),
            });
        } else if let Some(credits) =
            text.strip_prefix("NotifyTagMultiple(MISSION_REWARD_CREDITS): ")
        {
            // Newer builds omit the credit-bonus line; this tag is the same total.
            mission
                .credit_bonus
                .get_or_insert_with(|| credits.parse().unwrap_or(0));
        } else if text == "DBG: HudVis 0" {
            // Some mission types never print the credit bonus; the screen close is the last marker.
            let mission = self.mission.take().expect("mission checked above");
            events.push(Event::MissionEnd {
                line: self.line,
                ts_ms,
                success: mission.success,
                reward_tiers: mission.reward_tiers,
                credit_bonus: mission.credit_bonus.unwrap_or(0),
            });
        }
    }
}

// `<uptime>.<ms> <Subsystem> [<Level>]: <text>`; the uptime resets on every launch.
fn split(line: &str) -> Option<(u64, &str, &str)> {
    let (head, text) = line.split_once("]: ")?;
    let mut parts = head.split(' ');
    let ts_ms = parse_ts(parts.next()?)?;
    Some((ts_ms, parts.next()?, text))
}

// "Are you sure you want to equip Lith G3 Relic [RADIANT] for this mission? ..."
fn relic_equip(description: &str) -> Option<(String, String)> {
    let (_, tail) = description.split_once("you want to equip ")?;
    if !tail.contains(" for this mission?") {
        return None;
    }
    let (relic, state) = tail.split_once(" Relic [")?;
    let (state, _) = state.split_once(']')?;
    Some((relic.trim().to_string(), state.to_string()))
}

fn parse_ts(ts: &str) -> Option<u64> {
    // Warframe sometimes prefixes the timestamp with '!' (seen on the live
    // extraction marker); it is not part of the number.
    let ts = ts.trim_start_matches('!');
    let (secs, frac) = ts.split_once('.')?;
    let secs: u64 = secs.parse().ok()?;
    let frac: u64 = frac.parse().ok()?;
    let ms = match frac.checked_ilog10() {
        None => 0,
        Some(0) => frac * 100,
        Some(1) => frac * 10,
        Some(digits) => frac / 10u64.pow(digits - 2),
    };
    Some(secs * 1000 + ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_all(log: &str) -> Vec<Event> {
        let mut parser = Parser::new();
        let mut events = Vec::new();
        for line in log.lines() {
            events.extend(parser.push(line));
        }
        events.extend(parser.finish());
        events
    }

    #[test]
    fn parses_login_and_stamp() {
        let events = parse_all("12.253 Sys [Info]: Logged in TestTenno\n");
        assert_eq!(
            events,
            vec![Event::Login {
                line: 1,
                ts_ms: 12_253,
                user: "TestTenno".into()
            }]
        );
    }

    #[test]
    fn parses_relic_rewards_and_screen() {
        let log = "\
100.100 Script [Info]: OpenVoidProjectionRewardScreenRMI
100.280 Script [Info]: VoidProjections: 3 gets reward /Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/FulminPrimeBarrel
100.300 Script [Info]: VoidProjections: 1 gets reward /Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/SomeOtherPart
";
        let events = parse_all(log);
        assert_eq!(
            events,
            vec![
                Event::RelicExtraction {
                    line: 1,
                    ts_ms: 100_100
                },
                Event::RelicReward {
                    line: 2,
                    ts_ms: 100_280,
                    player_id: "3".into(),
                    game_ref:
                        "/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/FulminPrimeBarrel"
                            .into()
                },
                Event::RelicReward {
                    line: 3,
                    ts_ms: 100_300,
                    player_id: "1".into(),
                    game_ref: "/Lotus/StoreItems/Types/Recipes/Weapons/WeaponParts/SomeOtherPart"
                        .into()
                },
            ]
        );
    }

    #[test]
    fn parses_mission_end() {
        let log = "\
1032.704 Script [Info]: EndOfMatch.lua: Initialize
1032.705 Script [Info]: EndOfMatch.lua: Mission Succeeded
1032.705 Script [Info]: EndOfMatch.lua: EndOfMatch.lua: GiveMissionRewards. success=true
1032.706 Script [Info]: EndOfMatch.lua: RewardUtilities: Gave reward tier 0
1032.707 Script [Info]: EndOfMatch.lua: GetMissionCompleteCreditBonus: 1234
";
        assert_eq!(
            parse_all(log),
            vec![Event::MissionEnd {
                line: 5,
                ts_ms: 1_032_707,
                success: true,
                reward_tiers: vec![0],
                credit_bonus: 1234
            }]
        );
    }

    #[test]
    fn parses_context_block_into_loadout() {
        let log = "\
52.611 Game [Info]: Server::LoadClient: Context objects:
52.611 Game [Info]: /Lotus/Powersuits/Rhino/RhinoPrime
52.611 Game [Info]: /Lotus/Powersuits/Rhino/RhinoChargeAugmentCard
52.611 Game [Info]: /Lotus/Upgrades/Mods/Warframe/HealthPickupGivesArmourMod
52.611 Game [Info]: /Lotus/Upgrades/CosmeticEnhancers/Offensive/SecondaryCritOnHit
52.611 Game [Info]: /Lotus/Types/Restoratives/Cipher
52.611 Game [Info]: /Lotus/Weapons/Tenno/Pistols/PrimeKompressa/PrimeKompressa
52.611 Game [Info]: /Lotus/Types/Sentinels/SentinelAgent
52.611 Game [Info]: Level=/Lotus/Levels/Proc/Hub/Relay
";
        let events = parse_all(log);
        let [Event::Loadout { ts_ms, loadout, .. }] = events.as_slice() else {
            panic!("expected one loadout event, got {events:?}");
        };
        assert_eq!(*ts_ms, 52_611);
        assert_eq!(loadout.suits, ["/Lotus/Powersuits/Rhino/RhinoPrime"]);
        assert_eq!(
            loadout.mods,
            [
                "/Lotus/Powersuits/Rhino/RhinoChargeAugmentCard",
                "/Lotus/Upgrades/Mods/Warframe/HealthPickupGivesArmourMod"
            ]
        );
        assert_eq!(
            loadout.weapons,
            ["/Lotus/Weapons/Tenno/Pistols/PrimeKompressa/PrimeKompressa"]
        );
        assert_eq!(
            loadout.arcanes,
            ["/Lotus/Upgrades/CosmeticEnhancers/Offensive/SecondaryCritOnHit"]
        );
        assert_eq!(loadout.gear, ["/Lotus/Types/Restoratives/Cipher"]);
        assert_eq!(loadout.other, ["/Lotus/Types/Sentinels/SentinelAgent"]);
    }

    #[test]
    fn ignores_noise_lines() {
        assert!(parse_all("1.000 ResourceLoader [Info]: Found 1 items\n").is_empty());
        assert!(parse_all("not a log line\n").is_empty());
    }

    #[test]
    fn parses_the_relic_equip_dialog() {
        let log = "\
49.888 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to equip Lith G3 Relic [RADIANT] for this mission? It will be consumed if you seal the Void Fissure and extract., title= leftItem=/Menu/Confirm_Item_Yes, rightItem=/Menu/Confirm_Item_No)
";
        assert_eq!(
            parse_all(log),
            vec![Event::RelicEquipped {
                line: 1,
                ts_ms: 49_888,
                relic: "Lith G3".into(),
                state: "RADIANT".into(),
            }]
        );
    }

    #[test]
    fn parses_the_squad_size_hint() {
        let log = "\
945.277 Net [Info]: AddSquadMember: Squadmate3, mm=A39D73710D0D14A820EC2BE2, squadCount=4
945.278 Net [Info]: AddSquadMember: Broken, mm=A39D73710D0D14A820EC2BE2, squadCount=zero
";
        assert_eq!(
            parse_all(log),
            vec![Event::SquadSize {
                line: 1,
                ts_ms: 945_277,
                count: 4,
            }]
        );
    }

    #[test]
    fn parses_an_extraction_marker_with_a_bang_prefix() {
        let events = parse_all("!3435.176 Sys [Info]: VoidProjections: OpenVoidProjectionRewardScreen - PostMigration: 0\r\n");
        assert_eq!(
            events,
            vec![Event::RelicExtraction {
                line: 1,
                ts_ms: 3_435_176
            }]
        );
    }

    #[test]
    fn parses_the_rmi_extraction_marker_with_crlf() {
        let events = parse_all(
            "4494.762 Sys [Info]: VoidProjections: OpenVoidProjectionRewardScreenRMI\r\n",
        );
        assert_eq!(
            events,
            vec![Event::RelicExtraction {
                line: 1,
                ts_ms: 4_494_762
            }]
        );
    }

    #[test]
    fn parses_the_reward_screen_and_credit_tag() {
        let log = "\
154.764 Sys [Info]: VoidProjections: OpenVoidProjectionRewardScreen - PostMigration: 0
154.767 Script [Info]: EndOfMatch.lua: Initialize
154.767 Script [Info]: EndOfMatch.lua: Mission Succeeded
155.007 Script [Info]: EndOfMatch.lua: NotifyTagMultiple(MISSION_REWARD_CREDITS): 1100
181.773 Script [Info]: EndOfMatch.lua: DBG: HudVis 0
";
        assert_eq!(
            parse_all(log),
            vec![
                Event::RelicExtraction {
                    line: 1,
                    ts_ms: 154_764
                },
                Event::MissionEnd {
                    line: 5,
                    ts_ms: 181_773,
                    success: true,
                    reward_tiers: vec![],
                    credit_bonus: 1100
                }
            ]
        );
    }
}
