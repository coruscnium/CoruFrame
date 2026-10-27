use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeItem {
    pub name: String,
    pub quantity: u64,
    pub rank: Option<u64>,
    pub platinum: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeEvent {
    pub player: String,
    pub offering: Vec<TradeItem>,
    pub receiving: Vec<TradeItem>,
}

#[derive(Default)]
pub struct Parser {
    dialog: Option<String>,
    pending: Option<TradeEvent>,
}

impl Parser {
    pub fn feed(&mut self, line: &str) -> Option<TradeEvent> {
        let line = line.trim_end_matches('\r');
        if line.contains(
            "Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade?",
        ) {
            self.dialog = Some(line.to_string());
            self.pending = None;
            return None;
        }
        if let Some(dialog) = self.dialog.as_mut() {
            dialog.push('\n');
            dialog.push_str(line);
            if line.contains(", title= leftItem=") {
                let text = std::mem::take(dialog);
                self.dialog = None;
                self.pending = parse_dialog(&text);
            }
            return None;
        }
        if line.contains("Dialog.lua: Dialog::CreateOk(description=The trade was successful!") {
            return self.pending.take();
        }
        None
    }
}

fn parse_dialog(text: &str) -> Option<TradeEvent> {
    const OFFER: &str = "You are offering:";
    const RECEIVE: &str = "and will receive from ";
    const FOLLOWING: &str = " the following:";

    let offer_at = text.find(OFFER)? + OFFER.len();
    let receive_at = text.find(RECEIVE)?;
    if receive_at < offer_at {
        return None;
    }
    let after = &text[receive_at + RECEIVE.len()..];
    let following = after.find(FOLLOWING)?;
    let player = clean_player(&after[..following]);
    let receiving = after[following + FOLLOWING.len()..]
        .split(", title=")
        .next()
        .unwrap_or_default();
    Some(TradeEvent {
        player,
        offering: parse_items(&text[offer_at..receive_at]),
        receiving: parse_items(receiving),
    })
}

fn clean_player(text: &str) -> String {
    text.chars()
        .filter(|value| value.is_ascii_alphanumeric() || matches!(value, '_' | '-' | '.'))
        .collect()
}

fn parse_items(text: &str) -> Vec<TradeItem> {
    let mut items: Vec<TradeItem> = Vec::new();
    for raw in text.lines() {
        let line = raw
            .trim()
            .trim_start_matches('\u{feff}')
            .trim_end_matches(',')
            .trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Platinum x ") {
            push(
                &mut items,
                TradeItem {
                    name: "Platinum".into(),
                    quantity: parse_count(rest),
                    rank: None,
                    platinum: true,
                },
            );
            continue;
        }
        let (name, rank, quantity) = parse_item_line(line);
        if name.is_empty() {
            continue;
        }
        push(
            &mut items,
            TradeItem {
                name,
                quantity,
                rank,
                platinum: false,
            },
        );
    }
    items
}

fn parse_item_line(line: &str) -> (String, Option<u64>, u64) {
    let (line, quantity) = match line.rsplit_once(" x ") {
        Some((head, tail)) => (head, parse_count(tail).max(1)),
        None => (line, 1),
    };
    let Some(open) = line.rfind(" (") else {
        return (line.trim().to_string(), None, quantity);
    };
    if !line.ends_with(')') {
        return (line.trim().to_string(), None, quantity);
    }
    let inner = &line[open + 2..line.len() - 1];
    let mut words = inner.split_whitespace();
    let mut rank = None;
    while let Some(word) = words.next() {
        if word.eq_ignore_ascii_case("RANK") {
            rank = words
                .next()
                .and_then(|value| value.trim_end_matches(',').parse().ok());
            break;
        }
    }
    (line[..open].trim().to_string(), rank, quantity)
}

fn parse_count(text: &str) -> u64 {
    text.chars()
        .filter(|value| value.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

fn push(items: &mut Vec<TradeItem>, item: TradeItem) {
    if let Some(existing) = items.iter_mut().find(|current| {
        current.name == item.name && current.rank == item.rank && current.platinum == item.platinum
    }) {
        existing.quantity += item.quantity;
    } else {
        items.push(item);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(lines: &[&str]) -> Option<TradeEvent> {
        let mut parser = Parser::default();
        let mut event = None;
        for line in lines {
            if let Some(parsed) = parser.feed(line) {
                event = Some(parsed);
            }
        }
        event
    }

    #[test]
    fn parses_a_purchase_with_repeated_mods() {
        let event = parse(&[
            "1415.216 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:",
            "\rPlatinum x 5\r",
            "\r",
            "and will receive from ZeroFifteen\u{e000}\u{e001} the following:",
            "\u{feff}Ammo Drum (COMMON RANK 0)",
            "\u{feff}Ammo Drum (COMMON RANK 0)",
            "\u{feff}Ammo Drum (COMMON RANK 0)",
            "\u{feff}Ammo Drum (COMMON RANK 0)",
            "\u{feff}Ammo Drum (COMMON RANK 0)",
            "\u{feff}Ammo Drum (COMMON RANK 0), title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)",
            "1419.098 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)",
        ])
        .expect("trade event");

        assert_eq!(event.player, "ZeroFifteen");
        assert_eq!(
            event.offering,
            vec![TradeItem {
                name: "Platinum".into(),
                quantity: 5,
                rank: None,
                platinum: true,
            }]
        );
        assert_eq!(
            event.receiving,
            vec![TradeItem {
                name: "Ammo Drum".into(),
                quantity: 6,
                rank: Some(0),
                platinum: false,
            }]
        );
    }

    #[test]
    fn parses_a_sale_for_platinum() {
        let event = parse(&[
            "440.130 Script [Info]: Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:",
            "\rAmmo Case (COMMON RANK 0)",
            "\rAmmo Case (COMMON RANK 0)",
            "\rAmmo Case (COMMON RANK 0)",
            "\rAmmo Case (COMMON RANK 0)",
            "\rAmmo Case (COMMON RANK 0)",
            "\rAmmo Case (COMMON RANK 0)",
            "\r",
            "and will receive from Nydra0\u{e000} the following:",
            "\rPlatinum x 1, title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)",
            "445.795 Script [Info]: Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title= leftItem=/Menu/Confirm_Item_Ok)",
        ])
        .expect("trade event");

        assert_eq!(event.player, "Nydra0");
        assert_eq!(event.offering.len(), 1);
        assert_eq!(event.offering[0].name, "Ammo Case");
        assert_eq!(event.offering[0].quantity, 6);
        assert_eq!(event.offering[0].rank, Some(0));
        assert_eq!(event.receiving[0].name, "Platinum");
        assert_eq!(event.receiving[0].quantity, 1);
    }

    #[test]
    fn ignores_a_dialog_until_the_success_marker() {
        let mut parser = Parser::default();
        parser.feed("Dialog.lua: Dialog::CreateOkCancel(description=Are you sure you want to accept this trade? You are offering:");
        parser.feed("Platinum x 1");
        parser.feed("and will receive from Someone the following:");
        parser.feed("Ammo Drum (COMMON RANK 0), title= leftItem=/Menu/Confirm_Item_Ok, rightItem=/Menu/Confirm_Item_Cancel)");
        assert!(parser.pending.is_some());
        assert!(parser
            .feed("Dialog.lua: Dialog::CreateOk(description=The trade was successful!, title=")
            .is_some());
    }
}
