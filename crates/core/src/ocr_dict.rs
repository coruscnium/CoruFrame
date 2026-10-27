//! Tesseract user-words generated from the game data already on disk.
//!
//! A fixed engine model cannot know a new Prime part; the catalog and item DB
//! can. The list is deterministic (sorted, de-duplicated) so two builds from the
//! same data pass the same words to tesseract.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::catalog::Catalog;
use crate::fetch;
use crate::itemdb::ItemDb;
use crate::private_fs;
use crate::relic;

const MIN_WORD: usize = 2;

fn words(text: &str, out: &mut BTreeSet<String>) {
    for word in text.split(|character: char| !character.is_alphanumeric()) {
        if word
            .chars()
            .filter(|character| character.is_alphabetic())
            .count()
            < MIN_WORD
        {
            continue;
        }
        out.insert(word.to_string());
    }
}

pub fn user_words(catalog: &Catalog, db: Option<&ItemDb>, relics: &[relic::Entry]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for item in catalog.all() {
        words(item.name(), &mut out);
    }
    if let Some(db) = db {
        for entry in db.all() {
            words(&entry.name, &mut out);
        }
    }
    for entry in relics {
        for reward in &entry.rewards {
            words(&reward.item_name, &mut out);
        }
    }
    out.into_iter().collect()
}

pub fn ensure_user_words(
    catalog: &Catalog,
    db: Option<&ItemDb>,
    relics: &[relic::Entry],
) -> Option<PathBuf> {
    let words = user_words(catalog, db, relics);
    if words.is_empty() {
        return None;
    }
    let mut body = words.join("\n");
    body.push('\n');
    let path = fetch::cache_dir().join("ocr").join("eng.user-words");
    if std::fs::read_to_string(&path).is_ok_and(|existing| existing == body) {
        return Some(path);
    }
    match private_fs::write_private(&path, body.as_bytes()) {
        Ok(()) => Some(path),
        Err(error) => {
            eprintln!("ocr dictionary {}: {error}", path.display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_unique_sorted_and_drop_numbers() {
        let catalog = Catalog::from_json(
            r#"{"data":[
              {"slug":"braton_prime_barrel","gameRef":"","tags":[],"i18n":{"en":{"name":"Braton Prime Barrel"}}},
              {"slug":"axi_a5_relic","gameRef":"","tags":[],"i18n":{"en":{"name":"Axi A5 Relic"}}}
            ]}"#,
        )
        .unwrap();
        let words = user_words(&catalog, None, &[]);
        assert!(words.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(words.contains(&"Braton".to_string()));
        // "A5" has too few letters to be a word the model needs.
        assert!(!words.contains(&"A5".to_string()));
        assert!(words.contains(&"Relic".to_string()));
    }
}
