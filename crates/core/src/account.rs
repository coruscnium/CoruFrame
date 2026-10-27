use crate::hash::{fnv1a_64, hex16};

const LOGIN_MARKER: &str = "Sys [Info]: Logged in ";

// Warframe stamps some log fields with a zero-width character after the name;
// trimming only ASCII whitespace would keep it and change the hash.
fn is_invisible(character: char) -> bool {
    character.is_control()
        || character.is_whitespace()
        || matches!(character, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}')
}

pub fn alias_from_line(line: &str) -> Option<String> {
    let start = line.find(LOGIN_MARKER)? + LOGIN_MARKER.len();
    let alias = line[start..].trim_matches(is_invisible);
    if alias.is_empty() || alias.len() > 64 {
        return None;
    }
    Some(alias.to_string())
}

// A relaunched game appends a fresh login line; the last one is the live account.
pub fn alias_from_text(text: &str) -> Option<String> {
    text.lines().filter_map(alias_from_line).next_back()
}

// The hash is a carry key, not an authentication token: a different hash only
// disables carrying the previous process's optional arrays. It uses a fixed
// FNV-1a so a compiler or toolchain upgrade cannot silently change it.
pub fn id(alias: &str) -> String {
    hex16(fnv1a_64(alias.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_live_login_line() {
        let line = "12.466 Sys [Info]: Logged in SyntheticTenno";
        assert_eq!(alias_from_line(line).as_deref(), Some("SyntheticTenno"));
    }

    #[test]
    fn trims_invisible_characters() {
        let line = "12.466 Sys [Info]: Logged in \u{200b}SyntheticTenno\u{feff} \r";
        assert_eq!(alias_from_line(line).as_deref(), Some("SyntheticTenno"));
    }

    #[test]
    fn rejects_empty_and_unrelated_lines() {
        assert_eq!(alias_from_line("Sys [Info]: Logged in   "), None);
        assert_eq!(alias_from_line("Sys [Info]: Logged out"), None);
        let long = format!("Sys [Info]: Logged in {}", "a".repeat(65));
        assert_eq!(alias_from_line(&long), None);
    }

    #[test]
    fn last_login_wins() {
        let text = concat!(
            "1.000 Sys [Info]: Logged in First\n",
            "2.000 Sys [Info]: Logged in Second\r\n",
        );
        assert_eq!(alias_from_text(text).as_deref(), Some("Second"));
        assert_eq!(alias_from_text("nothing here"), None);
    }

    #[test]
    fn hashes_are_stable_per_alias() {
        assert_eq!(id("SyntheticTenno"), id("SyntheticTenno"));
        assert_ne!(id("SyntheticTenno"), id("Other"));
        assert_eq!(id("SyntheticTenno").len(), 16);
    }
}
