//! The HUD's corner and monitor, shared by the app's Settings page and the
//! overlay process. The app's localStorage is invisible to the overlay, so the
//! setting lives in a private file the overlay can read.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::private_fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "top-left" => Some(Self::TopLeft),
            "top-right" => Some(Self::TopRight),
            "bottom-left" => Some(Self::BottomLeft),
            "bottom-right" => Some(Self::BottomRight),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayConfig {
    pub corner: Corner,
    // A GDK connector name; absent means the first monitor, which GDK calls
    // the primary one.
    pub monitor: Option<String>,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            corner: Corner::TopRight,
            monitor: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Monitor {
    pub connector: String,
    pub model: String,
    pub primary: bool,
    pub width: i32,
    pub height: i32,
    pub scale: i32,
}

pub fn path() -> PathBuf {
    private_fs::config_dir().join("overlay.json")
}

pub fn load() -> OverlayConfig {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(config: &OverlayConfig) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(config).map_err(std::io::Error::other)?;
    private_fs::write_private(&path(), &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_round_trip_as_kebab_case() {
        for (corner, name) in [
            (Corner::TopLeft, "top-left"),
            (Corner::TopRight, "top-right"),
            (Corner::BottomLeft, "bottom-left"),
            (Corner::BottomRight, "bottom-right"),
        ] {
            assert_eq!(Corner::parse(name), Some(corner));
            let text = serde_json::to_string(&corner).unwrap();
            assert_eq!(text, format!("\"{name}\""));
        }
        assert_eq!(Corner::parse("middle"), None);
    }

    #[test]
    fn a_missing_or_broken_file_falls_back_to_the_default() {
        assert_eq!(OverlayConfig::default().corner, Corner::TopRight);
        assert_eq!(OverlayConfig::default().monitor, None);
    }

    #[test]
    fn the_config_round_trips_through_json() {
        let config = OverlayConfig {
            corner: Corner::BottomLeft,
            monitor: Some("DP-2".to_string()),
        };
        let text = serde_json::to_string(&config).unwrap();
        assert_eq!(
            serde_json::from_str::<OverlayConfig>(&text).unwrap(),
            config
        );
    }
}
