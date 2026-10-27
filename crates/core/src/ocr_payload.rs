//! Typed handoff between the reward probe and the HUD.
//!
//! The old handoff was a comma-joined list of names, so a card whose OCR failed
//! vanished instead of reaching the HUD as an unknown card. Every visible card
//! now crosses the boundary with its evidence crops, every OCR variant, the
//! chosen candidate and the confidence components. Crop paths are relative to
//! the payload's session directory; the loader refuses anything else.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::catalog::{ConfidenceTier, MatchEvidence};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewardPayload {
    pub session: String,
    pub frame: FrameInfo,
    pub cards: Vec<CardPayload>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameInfo {
    pub width: u32,
    pub height: u32,
    pub captured_unix_ms: u64,
    pub source: String,
    pub geometry: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CardPayload {
    pub index: u32,
    pub rect: Rect,
    #[serde(default)]
    pub card_crop: Option<PathBuf>,
    pub name_crop: PathBuf,
    #[serde(default)]
    pub ocr: Vec<OcrVariant>,
    #[serde(default)]
    pub chosen: Option<MatchedItem>,
    #[serde(default)]
    pub confidence: Confidence,
    #[serde(default)]
    pub price: Option<u32>,
    #[serde(default)]
    pub owned: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OcrVariant {
    pub engine: String,
    pub psm: u32,
    pub variant: String,
    pub text: String,
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchedItem {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub set: Option<String>,
    #[serde(default)]
    pub part: Option<String>,
    #[serde(default)]
    pub untradeable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Confidence {
    pub tier: ConfidenceTier,
    pub score: f32,
    pub evidence: MatchEvidence,
}

impl Default for Confidence {
    fn default() -> Self {
        Self {
            tier: ConfidenceTier::Low,
            score: 0.0,
            evidence: MatchEvidence::default(),
        }
    }
}

impl RewardPayload {
    // A payload is untrusted input once it sits on disk: a crop path may only be
    // one relative path component, so a corrupted payload cannot make the HUD
    // read an arbitrary file.
    pub fn resolve_asset(session_dir: &Path, asset: &Path) -> Option<PathBuf> {
        if asset.as_os_str().is_empty() {
            return None;
        }
        let mut parts = asset.components();
        let Component::Normal(name) = parts.next()? else {
            return None;
        };
        if parts.next().is_some() {
            return None;
        }
        Some(session_dir.join(name))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.cards.len() > 4 {
            return Err("payload has more than four cards".into());
        }
        for (position, card) in self.cards.iter().enumerate() {
            if card.index as usize != position {
                return Err(format!("card {} is out of order", card.index));
            }
            if Self::resolve_asset(Path::new(""), &card.name_crop).is_none() {
                return Err(format!("card {} has an unsafe name crop", card.index));
            }
            if let Some(crop) = &card.card_crop {
                if Self::resolve_asset(Path::new(""), crop).is_none() {
                    return Err(format!("card {} has an unsafe card crop", card.index));
                }
            }
        }
        Ok(())
    }

    pub fn write(&self, dir: &Path, path: &Path) -> Result<(), String> {
        self.validate()?;
        let json =
            serde_json::to_vec_pretty(self).map_err(|error| format!("payload json: {error}"))?;
        crate::private_fs::ensure_private_dir(dir)
            .map_err(|error| format!("session dir {}: {error}", dir.display()))?;
        crate::private_fs::write_private(path, &json)
            .map_err(|error| format!("write {}: {error}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let payload: Self = serde_json::from_str(&text)
            .map_err(|error| format!("payload {}: {error}", path.display()))?;
        payload.validate()?;
        Ok(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::MatchSource;

    fn payload() -> RewardPayload {
        RewardPayload {
            session: "test".into(),
            frame: FrameInfo {
                width: 3440,
                height: 1440,
                captured_unix_ms: 1,
                source: "portal".into(),
                geometry: "fixed".into(),
            },
            cards: vec![
                CardPayload {
                    index: 0,
                    rect: Rect {
                        left: 1032,
                        top: 532,
                        width: 344,
                        height: 144,
                    },
                    card_crop: Some("card-0.png".into()),
                    name_crop: "name-0.png".into(),
                    ocr: vec![OcrVariant {
                        engine: "tesseract".into(),
                        psm: 6,
                        variant: "luma".into(),
                        text: "Braton Prime Barrel".into(),
                        confidence: 89.0,
                        error: None,
                    }],
                    chosen: Some(MatchedItem {
                        slug: "braton_prime_barrel".into(),
                        name: "Braton Prime Barrel".into(),
                        set: Some("Braton Prime".into()),
                        part: Some("Braton Prime Barrel".into()),
                        untradeable: false,
                    }),
                    confidence: Confidence {
                        tier: ConfidenceTier::High,
                        score: 1.0,
                        evidence: MatchEvidence {
                            coverage: 1.0,
                            anchor_len: 6,
                            margin: 0.5,
                            unique: true,
                            variant_agreement: 1.0,
                            source: MatchSource::Global,
                            exact: true,
                        },
                    },
                    price: None,
                    owned: Some(1),
                },
                CardPayload {
                    index: 1,
                    rect: Rect {
                        left: 1376,
                        top: 532,
                        width: 344,
                        height: 144,
                    },
                    card_crop: None,
                    name_crop: "name-1.png".into(),
                    ocr: Vec::new(),
                    chosen: None,
                    confidence: Confidence::default(),
                    price: None,
                    owned: None,
                },
            ],
        }
    }

    #[test]
    fn round_trips_every_card_including_the_unknown() {
        let payload = payload();
        let json = serde_json::to_string(&payload).unwrap();
        let back: RewardPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload, back);
        assert!(back.cards[1].chosen.is_none());
        assert_eq!(back.cards[1].confidence.tier, ConfidenceTier::Low);
    }

    #[test]
    fn rejects_asset_paths_outside_the_session() {
        assert!(RewardPayload::resolve_asset(Path::new("/s"), Path::new("a.png")).is_some());
        assert!(RewardPayload::resolve_asset(Path::new("/s"), Path::new("../a.png")).is_none());
        assert!(RewardPayload::resolve_asset(Path::new("/s"), Path::new("/etc/passwd")).is_none());
        assert!(RewardPayload::resolve_asset(Path::new("/s"), Path::new("a/b.png")).is_none());
        assert!(RewardPayload::resolve_asset(Path::new("/s"), Path::new("")).is_none());
    }

    #[test]
    fn rejects_duplicate_or_out_of_order_cards() {
        let mut payload = payload();
        payload.cards[1].index = 2;
        assert!(payload.validate().is_err());
    }
}
