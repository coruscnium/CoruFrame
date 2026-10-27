//! `coruframe ocr-eval`: score the real probe pipeline against a local corpus.
//!
//! The manifest (names, resolutions, expected items) is committed; the frames
//! are not. The command runs the same geometry, variants, consensus matcher and
//! confidence tiers as the probe, so a passing corpus is evidence about the HUD
//! and not about a parallel test path.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use coruframe_core::catalog::ConfidenceTier;
use serde::{Deserialize, Serialize};

use crate::{Pools, Result};

#[derive(Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub frames: Vec<Frame>,
}

#[derive(Debug, Clone, Deserialize)]
struct Frame {
    pub file: String,
    #[serde(default)]
    pub width: Option<u32>,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub ui_scale: Option<u32>,
    #[serde(default = "english")]
    pub language: String,
    #[serde(default)]
    pub game_version: String,
    #[serde(default)]
    pub source: String,
    pub expected: Vec<String>,
    #[serde(default)]
    pub notes: String,
}

fn english() -> String {
    "en".to_string()
}

#[derive(Debug, Serialize)]
struct CardResult {
    index: usize,
    expected: Option<String>,
    predicted: Option<String>,
    tier: String,
    exact: bool,
    unknown: bool,
    wrong_price: bool,
}

#[derive(Debug, Serialize)]
struct FrameResult {
    file: String,
    width: u32,
    height: u32,
    ui_scale: u32,
    game_version: String,
    source: String,
    notes: String,
    expected_cards: usize,
    predicted_cards: usize,
    count_ok: bool,
    cards: Vec<CardResult>,
    latency_ms: u128,
}

#[derive(Debug, Default, Serialize)]
struct Metrics {
    frames: usize,
    cards: usize,
    exact: usize,
    unknown: usize,
    wrong_price: usize,
    count_ok: usize,
    name_rate: f64,
    unknown_rate: f64,
    p50_ms: f64,
    p95_ms: f64,
}

#[derive(Debug, Default, Serialize)]
struct TierMetrics {
    cards: usize,
    exact: usize,
    unknown: usize,
    wrong_price: usize,
}

#[derive(Debug, Serialize)]
struct Report {
    frames: Vec<FrameResult>,
    overall: Metrics,
    per_setup: BTreeMap<String, Metrics>,
    tiers: BTreeMap<String, TierMetrics>,
    gates: BTreeMap<String, bool>,
    passed: bool,
}

struct Args {
    corpus: PathBuf,
    manifest: PathBuf,
    json: bool,
    report: Option<PathBuf>,
    engine: String,
}

pub fn run(args: &[String]) -> Result<()> {
    let args = parse_args(args)?;
    if args.engine != "tesseract" {
        return Err(format!(
            "engine {:?} is not built into this binary; evaluate it behind the same interface first",
            args.engine
        )
        .into());
    }
    if !crate::ocr::available() {
        return Err("tesseract is not on PATH; install it or run inside the devshell".into());
    }
    let manifest = load_manifest(&args.manifest)?;
    if manifest.frames.is_empty() {
        return Err("manifest has no frames".into());
    }

    let catalog =
        coruframe_core::catalog::load(false).map_err(|error| format!("catalog: {error}"))?;
    let relics = coruframe_core::relic::load(false).unwrap_or_default();
    let db = coruframe_core::itemdb::ItemDb::load().ok();
    crate::ocr::set_user_words(coruframe_core::ocr_dict::ensure_user_words(
        &catalog,
        db.as_ref(),
        &relics,
    ));
    let pools = Pools {
        local: None,
        global: crate::reward_slugs_from(&catalog, &relics),
    };

    let mut frames = Vec::new();
    for frame in &manifest.frames {
        frames.push(evaluate_frame(&args.corpus, frame, &catalog, &pools)?);
    }
    let report = report(frames, &manifest);
    print_report(&report, args.json);
    if let Some(path) = &args.report {
        write_markdown(path, &report)?;
        eprintln!("ocr-eval: wrote {}", path.display());
    }
    if !report.passed {
        std::process::exit(1);
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Args> {
    let mut corpus = None;
    let mut manifest = None;
    let mut json = false;
    let mut report = None;
    let mut engine = "tesseract".to_string();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--corpus" => corpus = Some(PathBuf::from(iter.next().ok_or("--corpus needs a path")?)),
            "--manifest" => {
                manifest = Some(PathBuf::from(iter.next().ok_or("--manifest needs a path")?))
            }
            "--json" => json = true,
            "--report" => report = Some(PathBuf::from(iter.next().ok_or("--report needs a path")?)),
            "--engine" => engine = iter.next().ok_or("--engine needs a name")?.to_string(),
            other => return Err(format!("unknown option: {other}").into()),
        }
    }
    Ok(Args {
        corpus: corpus.ok_or("usage: coruframe ocr-eval --corpus DIR --manifest FILE [--json] [--report PATH] [--engine NAME]")?,
        manifest: manifest.ok_or("--manifest is required")?,
        json,
        report,
        engine,
    })
}

fn load_manifest(path: &Path) -> Result<Manifest> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    let manifest: Manifest = serde_json::from_str(&text)
        .map_err(|error| format!("manifest {}: {error}", path.display()))?;
    for frame in &manifest.frames {
        validate_file_name(&frame.file)?;
        if frame.expected.is_empty() {
            return Err(format!("{}: expected list is empty", frame.file).into());
        }
        if frame.language != "en" {
            return Err(format!("{}: only English frames are scored in 1.0", frame.file).into());
        }
    }
    Ok(manifest)
}

fn validate_file_name(name: &str) -> Result<()> {
    let path = Path::new(name);
    let mut parts = path.components();
    let Some(Component::Normal(_)) = parts.next() else {
        return Err(format!("corpus file {name:?} is not a plain file name").into());
    };
    if parts.next().is_some() || name.contains("..") {
        return Err(format!("corpus file {name:?} is not a plain file name").into());
    }
    Ok(())
}

fn evaluate_frame(
    corpus: &Path,
    frame: &Frame,
    catalog: &coruframe_core::catalog::Catalog,
    pools: &Pools,
) -> Result<FrameResult> {
    let path = corpus.join(&frame.file);
    let image = image::open(&path)
        .map_err(|error| format!("open {}: {error}", path.display()))?
        .to_rgb8();
    let (width, height) = image.dimensions();
    if let (Some(w), Some(h)) = (frame.width, frame.height) {
        if (w, h) != (width, height) {
            eprintln!(
                "ocr-eval: {} manifest says {w}x{h}, frame is {width}x{height}",
                frame.file
            );
        }
    }
    let started = Instant::now();
    let scan = crate::scan_image(catalog, &image, pools, &crate::crop_dir(), None)?;
    let latency_ms = started.elapsed().as_millis();

    let mut cards = Vec::new();
    for (index, card) in scan.cards.iter().enumerate() {
        let expected = frame.expected.get(index).cloned();
        let predicted = card.chosen.as_ref().map(|chosen| chosen.name.clone());
        let tier = card.confidence.tier;
        let exact =
            expected
                .as_deref()
                .zip(predicted.as_deref())
                .is_some_and(|(expected, predicted)| {
                    canonical_name(expected) == canonical_name(predicted)
                });
        let unknown = tier != ConfidenceTier::High;
        // Wrong price: a high-tier card whose item is not the card in that slot.
        let wrong_price = tier == ConfidenceTier::High
            && predicted.as_deref().zip(expected.as_deref()).is_some_and(
                |(predicted, expected)| canonical_name(predicted) != canonical_name(expected),
            );
        cards.push(CardResult {
            index,
            expected,
            predicted,
            tier: format!("{tier:?}").to_lowercase(),
            exact,
            unknown,
            wrong_price,
        });
    }

    Ok(FrameResult {
        file: frame.file.clone(),
        width,
        height,
        ui_scale: frame.ui_scale.unwrap_or(100),
        game_version: frame.game_version.clone(),
        source: frame.source.clone(),
        notes: frame.notes.clone(),
        expected_cards: frame.expected.len(),
        predicted_cards: scan.cards.len(),
        count_ok: scan.cards.len() == frame.expected.len(),
        cards,
        latency_ms,
    })
}

fn metrics(frames: &[&FrameResult]) -> Metrics {
    let mut metrics = Metrics {
        frames: frames.len(),
        ..Metrics::default()
    };
    let mut latencies = Vec::new();
    for frame in frames {
        metrics.count_ok += usize::from(frame.count_ok);
        latencies.push(frame.latency_ms as f64);
        for card in &frame.cards {
            metrics.cards += 1;
            metrics.exact += usize::from(card.exact);
            metrics.unknown += usize::from(card.unknown);
            metrics.wrong_price += usize::from(card.wrong_price);
        }
    }
    metrics.name_rate = if metrics.cards == 0 {
        0.0
    } else {
        metrics.exact as f64 / metrics.cards as f64
    };
    metrics.unknown_rate = if metrics.cards == 0 {
        0.0
    } else {
        metrics.unknown as f64 / metrics.cards as f64
    };
    metrics.p50_ms = percentile(&mut latencies.clone(), 0.50);
    metrics.p95_ms = percentile(&mut latencies, 0.95);
    metrics
}

fn percentile(values: &mut [f64], fraction: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() - 1) as f64 * fraction).round() as usize;
    values[index.min(values.len() - 1)]
}

fn report(frames: Vec<FrameResult>, manifest: &Manifest) -> Report {
    let all: Vec<&FrameResult> = frames.iter().collect();
    let overall = metrics(&all);
    let mut setups: BTreeMap<String, Vec<&FrameResult>> = BTreeMap::new();
    for frame in &frames {
        setups
            .entry(format!(
                "{}x{}@{}%",
                frame.width, frame.height, frame.ui_scale
            ))
            .or_default()
            .push(frame);
    }
    let per_setup = setups
        .into_iter()
        .map(|(key, frames)| (key, metrics(&frames)))
        .collect::<BTreeMap<_, _>>();
    // Calibration: how often each tier was right. A high tier with a wrong
    // price is the release blocker; medium/low are allowed to be unknown.
    let mut tiers: BTreeMap<String, TierMetrics> = BTreeMap::new();
    for frame in &frames {
        for card in &frame.cards {
            let tier = tiers.entry(card.tier.clone()).or_default();
            tier.cards += 1;
            tier.exact += usize::from(card.exact);
            tier.unknown += usize::from(card.unknown);
            tier.wrong_price += usize::from(card.wrong_price);
        }
    }

    let mut gates = BTreeMap::new();
    gates.insert(
        "count".into(),
        overall.count_ok == overall.frames && !frames.is_empty(),
    );
    gates.insert("name".into(), overall.name_rate >= 0.995);
    gates.insert(
        "name_per_setup".into(),
        per_setup.values().all(|metrics| metrics.name_rate >= 0.98),
    );
    gates.insert("wrong_price".into(), overall.wrong_price == 0);
    gates.insert("unknown".into(), overall.unknown_rate <= 0.02);
    gates.insert("latency".into(), overall.p95_ms <= 2000.0);
    let passed = gates.values().all(|value| *value);
    eprintln!(
        "ocr-eval: {} frame(s), manifest v{}, {} card(s)",
        frames.len(),
        manifest.version,
        overall.cards
    );
    Report {
        frames,
        overall,
        per_setup,
        tiers,
        gates,
        passed,
    }
}

fn print_report(report: &Report, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(report).unwrap_or_default()
        );
        return;
    }
    for frame in &report.frames {
        println!(
            "{} {}x{} @{}%: {}/{} card(s) ({}) {}ms",
            frame.file,
            frame.width,
            frame.height,
            frame.ui_scale,
            frame.predicted_cards,
            frame.expected_cards,
            if frame.count_ok {
                "count ok"
            } else {
                "COUNT MISMATCH"
            },
            frame.latency_ms
        );
        for card in &frame.cards {
            println!(
                "  {} expected {:?} -> {:?} [{}]{}",
                card.index,
                card.expected.as_deref().unwrap_or("-"),
                card.predicted.as_deref().unwrap_or("-"),
                card.tier,
                if card.wrong_price { " WRONG PRICE" } else { "" }
            );
        }
    }
    let overall = &report.overall;
    println!(
        "overall: exact {:.2}% ({} / {}), unknown {:.2}%, wrong price {}, p50 {}ms, p95 {}ms",
        overall.name_rate * 100.0,
        overall.exact,
        overall.cards,
        overall.unknown_rate * 100.0,
        overall.wrong_price,
        overall.p50_ms,
        overall.p95_ms
    );
    for (tier, metrics) in &report.tiers {
        println!(
            "tier {tier:<8} {:>3} card(s), {} exact, {} unknown, {} wrong price",
            metrics.cards, metrics.exact, metrics.unknown, metrics.wrong_price
        );
    }
    for (name, passed) in &report.gates {
        println!("gate {name:<16} {}", if *passed { "pass" } else { "FAIL" });
    }
    println!("ocr-eval: {}", if report.passed { "pass" } else { "FAIL" });
}

fn write_markdown(path: &Path, report: &Report) -> Result<()> {
    let mut text = String::new();
    text.push_str("# OCR corpus report\n\n");
    text.push_str("| frame | size | expected | predicted | count | p50 |\n");
    text.push_str("|---|---|---|---|---|---|\n");
    for frame in &report.frames {
        text.push_str(&format!(
            "| {} | {}x{} | {} | {} | {} | {} ms |\n",
            frame.file,
            frame.width,
            frame.height,
            frame.expected_cards,
            frame.predicted_cards,
            if frame.count_ok { "ok" } else { "mismatch" },
            frame.latency_ms
        ));
        if !frame.notes.is_empty() {
            text.push_str(&format!(
                "\n{}: build {}, source `{}`, {}\n\n",
                frame.file, frame.game_version, frame.source, frame.notes
            ));
        }
    }
    text.push_str(&format!(
        "\nExact {:.2}% ({} / {}), unknown {:.2}%, wrong price {}, p50 {} ms, p95 {} ms.\n",
        report.overall.name_rate * 100.0,
        report.overall.exact,
        report.overall.cards,
        report.overall.unknown_rate * 100.0,
        report.overall.wrong_price,
        report.overall.p50_ms,
        report.overall.p95_ms
    ));
    text.push_str("\n| tier | cards | exact | unknown | wrong price |\n|---|---|---|---|---|\n");
    for (tier, metrics) in &report.tiers {
        text.push_str(&format!(
            "| {tier} | {} | {} | {} | {} |\n",
            metrics.cards, metrics.exact, metrics.unknown, metrics.wrong_price
        ));
    }
    text.push_str("\n| gate | result |\n|---|---|\n");
    for (name, passed) in &report.gates {
        text.push_str(&format!(
            "| {name} | {} |\n",
            if *passed { "pass" } else { "FAIL" }
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    coruframe_core::private_fs::write_private(path, text.as_bytes())
        .map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(())
}

fn canonical_name(text: &str) -> String {
    text.chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_manifest_parses_and_is_complete() {
        let manifest: Manifest =
            serde_json::from_str(include_str!("../tests/ocr/manifest.json")).unwrap();
        assert!(!manifest.frames.is_empty());
        for frame in &manifest.frames {
            validate_file_name(&frame.file).unwrap();
            assert!(!frame.expected.is_empty(), "{}", frame.file);
            assert_eq!(frame.language, "en");
        }
    }

    #[test]
    fn names_compare_on_case_and_punctuation_only() {
        assert_eq!(
            canonical_name("Braton Prime Barrel"),
            canonical_name("braton  prime-barrel")
        );
        assert_ne!(
            canonical_name("Braton Prime Barrel"),
            canonical_name("Braton Prime Receiver")
        );
    }

    #[test]
    fn percentile_uses_the_upper_rank() {
        let mut values = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(percentile(&mut values, 0.5), 30.0);
        assert_eq!(percentile(&mut values, 0.95), 50.0);
    }

    #[test]
    fn a_missing_or_nested_frame_name_is_rejected() {
        assert!(validate_file_name("../frame.png").is_err());
        assert!(validate_file_name("dir/frame.png").is_err());
        assert!(validate_file_name("frame.png").is_ok());
    }
}
