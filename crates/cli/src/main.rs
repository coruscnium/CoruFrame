use std::collections::{BTreeMap, HashMap, HashSet};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod capture;
mod doctor;
mod hud;
mod kwinshot;
mod monitors;
mod ocr;
mod ocr_eval;
mod portal;
mod reward;
mod snapshot;
mod stream;

use capture::Backend;
use coruframe_core::catalog::{self, ConfidenceTier, MatchEvidence, MatchSource};
use coruframe_core::fetch;
use coruframe_core::itemdb;
use coruframe_core::ledger::Ledger;
use coruframe_core::logpath;
use coruframe_core::mastery::Mastery;
use coruframe_core::ocr_payload::{
    CardPayload, Confidence, FrameInfo, MatchedItem, OcrVariant, Rect, RewardPayload,
};
use coruframe_core::parse::{Event, Parser};
use coruframe_core::price;
use coruframe_core::private_fs;
use coruframe_core::private_fs::append_private;
use coruframe_core::relic;
use coruframe_core::session::{self, Tracker};
use coruframe_core::tail::Follower;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const REWARD_WINDOW: Duration = Duration::from_secs(20);
// The cards follow the extraction marker by ~4-6 s. The first capture is
// strict: it only opens the HUD when every expected card is high confidence.
// A later capture accepts unknowns, so an early attempt cannot drop a card and
// a slow frame still gets the full reward window.
const HUD_CAPTURE_DELAY: Duration = Duration::from_secs(4);
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const KEEP_FRAMES: usize = 10;
const SESSION_TTL: Duration = Duration::from_secs(3600);
const DEBUG_FRAME_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const OCR_LOG_MAX_BYTES: u64 = 2 * 1024 * 1024;
// The reward screen lives for 20 s. Cards and evidence have to be on screen
// well before that, so a slow machine stops adding variants and layouts instead
// of spending the window inside tesseract.
const OCR_BUDGET: Duration = Duration::from_secs(6);

fn main() -> Result<()> {
    coruframe_core::capabilities::prepare_wayland();
    // Restore the default so `coruframe ... | head` exits quietly instead of panicking.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    for skipped in private_fs::harden_existing_paths() {
        eprintln!("permissions: {skipped}");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => println!("coruframe {}", env!("CARGO_PKG_VERSION")),
        Some("logpath") => match logpath::find() {
            Some(path) => println!("{}", path.display()),
            None => {
                eprintln!("EE.log not found");
                std::process::exit(2);
            }
        },
        Some("scan") => scan(&args[1..])?,
        Some("watch") => watch(&args[1..])?,
        Some("probe") => probe(&args[1..])?,
        Some("capture") => {
            coruframe_core::capabilities::require_wayland()?;
            capture_cmd(&args[1..])?
        }
        Some("capture-source") => portal::choose(&args[1..])?,
        Some("capabilities") => capabilities_cmd(&args[1..])?,
        Some("monitors") => monitors::run(&args[1..])?,
        Some("hud") => {
            coruframe_core::capabilities::require_wayland()?;
            hud::run(&args[1..])?
        }
        Some("ocr-eval") => ocr_eval::run(&args[1..])?,
        Some("ocr-crops") => ocr_crops(&args[1..])?,
        Some("doctor") => doctor::run(&args[1..])?,
        Some("install-desktop") => install_desktop(&args[1..])?,
        Some("launch") => {
            coruframe_core::capabilities::require_wayland()?;
            snapshot::launch(&args[1..])?
        }
        Some("price") => price_cmd(&args[1..])?,
        Some("relic") => relic_cmd(&args[1..])?,
        Some("catalog") => catalog_cmd(&args[1..])?,
        Some("itemdb") => itemdb_cmd(&args[1..])?,
        Some("help" | "--help" | "-h") | None => usage(),
        Some(command) => {
            eprintln!("unknown command: {command}");
            usage();
            std::process::exit(2);
        }
    }
    Ok(())
}

fn usage() {
    println!(
        "coruframe <command>

  logpath                                   print the EE.log path
  scan  [--log PATH] [--db PATH] [--json]   replay the whole log into the ledger
  watch [--log PATH] [--db PATH] [--json]   tail the log and print new events (250 ms)
  probe [--log PATH] [--out DIR] [--now]
        [--image FRAME] [--hud] [--log-file PATH]
                                             capture + OCR and draw the reward HUD on a relic reveal
  capture [--out DIR] [--backend auto|kwin|wlr|x11|portal]
                                             capture one verified frame and print its path
  capture-source [--json]                    ask the portal to choose monitor or window
  capabilities [--json]                      print what this system can do (no dialog)
  monitors [--json]                          list GDK monitor connectors for the HUD
  hud (--mock PATH [--assets DIR] | --payload PATH) [--no-prices] [--seconds N]
                                             draw the reward HUD from a mock payload or a card payload
  ocr-eval --corpus DIR --manifest FILE [--json] [--report PATH]
                                             score the OCR pipeline against a local frame corpus
  ocr-crops --frame PATH                     export the card and name-strip PNGs for a frame
  doctor [--report]                          print session, capabilities, paths and permission state
  install-desktop [--dir DIR]                write the KWin ScreenShot2 allowlist entry
  launch [--settle N] [--scan-every N] -- <command>
                                             wrap the game as its ancestor and write an
                                             opt-in, read-only inventory scan report
  price <item>                              lowest online sell price (name, slug or /Lotus path)
  relic <tier> <name> [--price]             drop table with ducat and platinum expectation
  catalog update                            refresh the item, relic and worldstate caches
  itemdb update                             build the full WFCD item database cache
  itemdb audit [--snapshot PATH]            resolve the inventory snapshot against the item DB"
    );
}

struct Opts {
    log: Option<PathBuf>,
    db: Option<PathBuf>,
    json: bool,
    now: bool,
    out: Option<PathBuf>,
    image: Option<PathBuf>,
    log_file: Option<PathBuf>,
}

impl Opts {
    fn parse(args: &[String]) -> Result<Self> {
        let mut opts = Opts {
            log: None,
            db: None,
            json: false,
            now: false,
            out: None,
            image: None,
            log_file: None,
        };
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--json" => opts.json = true,
                "--now" => opts.now = true,
                // The HUD is the only live path; the flag stays accepted so the
                // documented `probe --hud` command keeps working.
                "--hud" => {}
                "--log" => opts.log = Some(args.next().ok_or("--log needs a path")?.into()),
                "--db" => opts.db = Some(args.next().ok_or("--db needs a path")?.into()),
                "--out" => opts.out = Some(args.next().ok_or("--out needs a path")?.into()),
                "--image" => opts.image = Some(args.next().ok_or("--image needs a path")?.into()),
                "--log-file" => {
                    opts.log_file = Some(args.next().ok_or("--log-file needs a path")?.into())
                }
                other => return Err(format!("unknown option: {other}").into()),
            }
        }
        Ok(opts)
    }
}

fn scan(args: &[String]) -> Result<()> {
    let opts = Opts::parse(args)?;
    let path = log_path(opts.log)?;
    let bytes = std::fs::read(&path)?;
    let text = String::from_utf8_lossy(&bytes);
    let session = session::from_text(&text).unwrap_or_default();
    let ledger = open_ledger(&opts.db)?;
    let mut parser = Parser::new();

    for line in text.lines() {
        for event in parser.push(line) {
            emit(&event, opts.json);
            ledger.record(&session, &event)?;
        }
    }
    for event in parser.finish() {
        emit(&event, opts.json);
        ledger.record(&session, &event)?;
    }

    if !opts.json {
        for (game_ref, count) in ledger.counts()? {
            println!("{count:>6}  {game_ref}");
        }
    }
    Ok(())
}

fn watch(args: &[String]) -> Result<()> {
    let opts = Opts::parse(args)?;
    let path = log_path(opts.log)?;
    let ledger = open_ledger(&opts.db)?;
    let (mut tracker, existing_lines) = read_start(&path);
    let mut parser = Parser::starting_after(existing_lines);
    let mut follower = Follower::new(&path, false);

    loop {
        match follower.poll() {
            Ok(lines) => {
                if follower.last_poll_truncated() {
                    // A new launch: the header may not be written yet, so the key is resolved lazily.
                    parser = Parser::new();
                    tracker = Tracker::new(None);
                }
                for line in lines {
                    let ready = tracker.push(line);
                    if ready.is_empty() {
                        continue;
                    }
                    let session = tracker.key().unwrap_or_default().to_string();
                    for line in ready {
                        for event in parser.push(&line) {
                            emit(&event, opts.json);
                            ledger.record(&session, &event)?;
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn capabilities_cmd(args: &[String]) -> Result<()> {
    match args {
        [] => {}
        [flag] if flag == "--json" => {}
        [other, ..] => return Err(format!("unknown option: {other}").into()),
    }
    let mut caps = coruframe_core::capabilities::detect();
    caps.layer_shell = doctor::layer_shell();
    println!("{}", serde_json::to_string_pretty(&caps)?);
    Ok(())
}

fn capture_cmd(args: &[String]) -> Result<()> {
    let mut dir = std::env::temp_dir();
    let mut backend = capture::Backend::Auto;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--out" => dir = iter.next().ok_or("--out needs a path")?.into(),
            "--backend" => {
                backend = backend_of(iter.next().ok_or("--backend needs a value")?)?;
            }
            other => return Err(format!("unknown option: {other}").into()),
        }
    }
    let frame = capture::capture(&dir, backend)?;
    println!("{}", frame.path.display());
    Ok(())
}

fn backend_of(value: &str) -> Result<Backend> {
    Backend::parse(value)
        .ok_or_else(|| format!("unknown backend: {value} (auto, kwin, wlr, x11, portal)").into())
}

fn probe(args: &[String]) -> Result<()> {
    let opts = Opts::parse(args)?;
    let dir = opts
        .out
        .unwrap_or_else(|| fetch::cache_dir().join("frames"));
    let backend = Backend::Auto;
    let log_file = opts
        .log_file
        .unwrap_or_else(|| fetch::cache_dir().join("probe.log"));
    let mut stage = Stage::open(&log_file);
    stage.log(&format!(
        "start backend={} out={} log={}",
        backend.name(),
        dir.display(),
        log_file.display()
    ));

    // Without a catalog the gate can never pass; fail now instead of arming silently.
    let catalog = match catalog::load(false) {
        Ok(catalog) => catalog,
        Err(error) => {
            stage.log(&format!("catalog error: {error}"));
            return Err(error.into());
        }
    };
    let relics = relic::load(false).unwrap_or_else(|error| {
        stage.log(&format!("relics: {error}"));
        Vec::new()
    });
    let rewards = reward_slugs_from(&catalog, &relics);
    match &rewards {
        Some(slugs) => stage.log(&format!("reward pool: {} item(s)", slugs.len())),
        None => stage.log("reward pool unavailable; matching the full catalog"),
    }
    let engine_ready = ocr::available();
    stage.log(&format!(
        "ocr engine: {}",
        if engine_ready { "tesseract" } else { "missing" }
    ));
    if !engine_ready {
        return Err("tesseract is not on PATH; the reward probe needs it".into());
    }
    let db = itemdb::ItemDb::load().ok();
    ocr::set_user_words(coruframe_core::ocr_dict::ensure_user_words(
        &catalog,
        db.as_ref(),
        &relics,
    ));
    sweep_sessions();
    sweep_debug_frames();
    rotate_log(&fetch::cache_dir().join("ocr.log"));
    rotate_log(&fetch::cache_dir().join("hud.log"));

    if let Some(image) = &opts.image {
        stage.log(&format!("image {}", image.display()));
        if let Err(error) = probe_image(&catalog, rewards.as_ref(), image, &mut stage) {
            stage.log(&format!("error: {error}"));
            return Err(error.into());
        }
        return Ok(());
    }

    if opts.now {
        let started = Instant::now();
        let frame = capture::capture(&dir, backend)?;
        stage.log(&format!(
            "capture {}ms {} {}",
            started.elapsed().as_millis(),
            frame.path.display(),
            size_text(Some((frame.width, frame.height)))
        ));
        println!("captured {}", frame.path.display());
        let ocr_started = Instant::now();
        match ocr::ocr_tsv(&frame.path, 6) {
            Ok(captured) => {
                stage.log(&format!(
                    "ocr {}ms, {} word(s)",
                    ocr_started.elapsed().as_millis(),
                    captured.words.len()
                ));
                print_ocr(&captured.text);
            }
            Err(error) => {
                stage.log(&format!("ocr error: {error}"));
                eprintln!("ocr: {error}");
            }
        }
        return Ok(());
    }

    // The live path is the portal HUD; --image and --now do not need a grant.
    // One probe owns the log and the portal grant. A second instance exits
    // instead of stacking another HUD and another screen request.
    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
        if libc::getppid() == 1 {
            stage.log("parent is gone; exiting");
            return Ok(());
        }
    }
    let _probe_lock = acquire_probe_lock(&mut stage)?;
    if std::env::var_os("CORUFRAME_CAPTURE_CMD").is_some() {
        stage.log("capture: command seam");
    } else if let Err(error) = portal::ensure_token() {
        stage.log(&format!("capture grant error: {error}"));
        eprintln!("capture: {error}");
        return Err(error);
    } else {
        stage.log("capture grant ready");
    }

    let path = log_path(opts.log)?;
    let first_line = count_lines(&path);
    stage.log(&format!(
        "watching {} from line {first_line}",
        path.display()
    ));
    let mut follower = Follower::new(&path, false);
    let mut parser = Parser::starting_after(first_line);
    let mut arm = ArmState::default();
    let mut equipped_relic: Option<String> = None;
    let mut squad_size: Option<u32> = None;
    let mut prefetched: HashSet<String> = HashSet::new();
    prune_frames(&dir);
    eprintln!("watching {}", path.display());
    let mut last_beat = Instant::now();

    loop {
        match follower.poll() {
            Ok(lines) => {
                if follower.last_poll_truncated() {
                    parser = Parser::new();
                    arm.disarm();
                    stage.log("log truncated: parser reset and disarmed");
                }
                for line in lines {
                    for event in parser.push(&line) {
                        match &event {
                            Event::RelicEquipped { relic, .. } => {
                                equipped_relic = Some(relic.clone());
                                stage.log(&format!("relic equipped: {relic}"));
                            }
                            Event::SquadSize { count, .. } => {
                                squad_size = Some(*count);
                                stage.log(&format!("squad size: {count}"));
                            }
                            Event::RelicExtraction { line, ts_ms } => {
                                // Extraction starts ~5-6 s before the reward cards appear.
                                let now = Instant::now();
                                arm.arm_delayed(now, HUD_CAPTURE_DELAY);
                                let local = equipped_relic
                                    .as_deref()
                                    .and_then(|name| local_relic_pool(&catalog, &relics, name));
                                stage.log(&format!(
                                    "local reward pool: {} item(s)",
                                    local.as_ref().map_or(0, HashSet::len)
                                ));
                                prefetch_outcomes(
                                    &catalog,
                                    &relics,
                                    equipped_relic.as_deref(),
                                    &mut prefetched,
                                );
                                stage.log(&format!(
                                    "armed at line {line} ts={ts_ms} window={}s",
                                    REWARD_WINDOW.as_secs()
                                ));
                                println!(
                                    "armed: reward screen expected within {} s",
                                    REWARD_WINDOW.as_secs()
                                );
                            }
                            _ => {}
                        }
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                stage.log(&format!("log error: {error}"));
                return Err(error.into());
            }
        }

        let now = Instant::now();
        if arm.expired(now) {
            arm.disarm();
            stage.log("disarmed: window expired");
            eprintln!("disarmed: no reward screen found");
        } else if arm.next_capture(now) {
            let started = Instant::now();
            let pools = Pools {
                local: equipped_relic
                    .as_deref()
                    .and_then(|name| local_relic_pool(&catalog, &relics, name)),
                global: rewards.clone(),
            };
            let strict = arm.attempts == 1;
            match hud_capture(&catalog, &pools, &dir, squad_size, db.as_ref(), strict) {
                Ok(Some(pid)) => {
                    let message = format!("hud {}ms pid {pid}", started.elapsed().as_millis());
                    stage.log(&message);
                    println!("{message}");
                    arm.disarm();
                }
                Ok(None) => {
                    stage.log("hud: no reward text");
                    println!("no reward text");
                }
                Err(error) => {
                    stage.log(&format!("hud error: {error}"));
                    eprintln!("hud: {error}");
                }
            }
        }

        if last_beat.elapsed() >= Duration::from_secs(10) {
            let lines = count_lines(&path);
            stage.log(&format!("heartbeat: {lines} lines"));
            eprintln!("heartbeat: {lines} lines");
            last_beat = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn probe_image_frame(dir: &Path) -> std::result::Result<image::RgbImage, String> {
    // Capture seam: CORUFRAME_CAPTURE_CMD writes
    // the frame to CORUFRAME_OUT, so OCR and the HUD can be tested without the game.
    if std::env::var_os("CORUFRAME_CAPTURE_CMD").is_some() {
        let frame = capture::capture(dir, Backend::Auto)?;
        let image = image::open(&frame.path)
            .map_err(|error| format!("open {}: {error}", frame.path.display()))?
            .to_rgb8();
        let _ = std::fs::remove_file(&frame.path);
        return Ok(image);
    }
    let frame = portal::capture_frame().map_err(|error| error.to_string())?;
    frame
        .rgb_image()
        .ok_or_else(|| "frame size mismatch".to_string())
}

#[derive(Clone, Default)]
struct Pools {
    local: Option<HashSet<String>>,
    global: Option<HashSet<String>>,
}

#[derive(Clone)]
struct CardScan {
    crop: reward::Crop,
    chosen: Option<MatchedItem>,
    confidence: Confidence,
}

struct Scan {
    cards: Vec<CardScan>,
    geometry: reward::GeometrySource,
}

#[derive(Clone, Copy)]
struct RowGeometry {
    top: Option<u32>,
    source: reward::GeometrySource,
}

fn match_text<'a>(
    catalog: &'a catalog::Catalog,
    text: &str,
    pools: &Pools,
) -> Option<catalog::MatchResult<'a>> {
    if let Some(local) = &pools.local {
        if let Some(found) = catalog.best_reward_match_in(text, Some(local), MatchSource::Local) {
            return Some(found);
        }
    }
    if let Some(global) = &pools.global {
        if let Some(found) = catalog.best_reward_match_in(text, Some(global), MatchSource::Global) {
            return Some(found);
        }
    }
    catalog.best_reward_match_in(text, None, MatchSource::Full)
}

// Consensus over every OCR variant: the slug with the most agreeing variants
// wins, and the agreement becomes part of the confidence. A single variant that
// splits its vote cannot reach the high tier.
fn match_crop(
    catalog: &catalog::Catalog,
    crop: &reward::Crop,
    pools: &Pools,
) -> (Option<MatchedItem>, Confidence) {
    let texts: Vec<&reward::OcrText> = crop
        .texts
        .iter()
        .filter(|text| !text.text.trim().is_empty())
        .collect();
    if texts.is_empty() {
        return (None, Confidence::default());
    }
    // Forma has no market entry, so one variant saying "forma" is not enough:
    // require the same majority rule the priced cards use.
    let forma_votes = texts
        .iter()
        .filter_map(|text| catalog::untradeable(&text.text))
        .collect::<Vec<_>>();
    if forma_votes.len() * 2 >= texts.len() {
        let name = forma_votes[0];
        let agreement = forma_votes.len() as f32 / texts.len() as f32;
        let evidence = MatchEvidence {
            coverage: 1.0,
            anchor_len: name.len() as u32,
            margin: 1.0,
            unique: true,
            variant_agreement: agreement,
            source: MatchSource::Untradeable,
            exact: true,
        };
        return (
            Some(MatchedItem {
                slug: format!("untradeable:{}", catalog::normalize_ocr(name)),
                name: name.to_string(),
                set: None,
                part: None,
                untradeable: true,
            }),
            Confidence {
                tier: evidence.tier(),
                score: agreement,
                evidence,
            },
        );
    }

    let mut votes: Vec<(String, usize, catalog::MatchResult<'_>)> = Vec::new();
    for text in &texts {
        let Some(found) = match_text(catalog, &text.text, pools) else {
            continue;
        };
        if let Some((_, count, _)) = votes
            .iter_mut()
            .find(|(slug, _, _)| *slug == found.item.slug)
        {
            *count += 1;
        } else {
            votes.push((found.item.slug.clone(), 1, found));
        }
    }
    let Some((slug, count, found)) = votes.into_iter().max_by(|left, right| {
        left.1.cmp(&right.1).then(
            left.2
                .evidence
                .coverage
                .total_cmp(&right.2.evidence.coverage),
        )
    }) else {
        return (
            None,
            Confidence {
                tier: ConfidenceTier::Low,
                score: 0.0,
                evidence: MatchEvidence::default(),
            },
        );
    };
    let agreement = count as f32 / texts.len() as f32;
    let mut evidence = found.evidence.clone();
    evidence.variant_agreement = agreement;
    let tier = evidence.tier();
    let score = evidence.coverage * agreement;
    let chosen = MatchedItem {
        slug,
        name: found.item.name().to_string(),
        set: None,
        part: None,
        untradeable: false,
    };
    (
        Some(chosen),
        Confidence {
            tier,
            score,
            evidence,
        },
    )
}

fn scan_image(
    catalog: &catalog::Catalog,
    image: &image::RgbImage,
    pools: &Pools,
    crop_dir: &Path,
    hint: Option<u32>,
) -> std::result::Result<Scan, String> {
    private_fs::ensure_private_dir(crop_dir).map_err(|error| format!("crops: {error}"))?;
    let (_, height) = image.dimensions();
    let detection = reward::detect_row(image);
    let fixed_top = reward::fixed_top(height);
    let (detected, source) = match detection {
        Some((top, _)) if top.abs_diff(fixed_top) <= height / 100 => {
            (Some(top), reward::GeometrySource::Detected)
        }
        _ => (None, reward::GeometrySource::Fixed),
    };
    let row = RowGeometry {
        top: detected,
        source,
    };
    let started = Instant::now();
    match hint {
        Some(count) if (1..=4).contains(&count) => {
            scan_hinted(catalog, image, pools, crop_dir, count, row, started)
        }
        _ => scan_unhinted(catalog, image, pools, crop_dir, row, started),
    }
}

// No squad hint: score every layout as before. This is the path used by
// `probe --image` and the existing OCR corpus.
fn scan_unhinted(
    catalog: &catalog::Catalog,
    image: &image::RgbImage,
    pools: &Pools,
    crop_dir: &Path,
    row: RowGeometry,
    started: Instant,
) -> std::result::Result<Scan, String> {
    let mut best: Option<(f32, Scan)> = None;
    for cards in reward::LAYOUTS {
        if started.elapsed() >= OCR_BUDGET {
            break;
        }
        let mut scan = Scan {
            cards: scan_layout_luma(catalog, image, pools, crop_dir, cards, row.top)?,
            geometry: row.source,
        };
        // Only the four-card first pass gets the extra variants, and only for
        // cards that did not reach the high tier.
        if cards == 4 {
            rescue_layout(&mut scan, catalog, pools, crop_dir, started);
        }
        let chosen = scan
            .cards
            .iter()
            .filter(|card| card.chosen.is_some())
            .count();
        let unknown = scan.cards.len().saturating_sub(chosen);
        let score = scan
            .cards
            .iter()
            .filter_map(|card| card.chosen.as_ref().map(|_| card.confidence.score))
            .sum::<f32>()
            - 1.5 * unknown as f32;
        if cards == 4 && chosen >= 3 {
            return Ok(scan);
        }
        if best
            .as_ref()
            .is_none_or(|(best_score, _)| score > *best_score)
        {
            best = Some((score, scan));
        }
        let smaller_max = cards.saturating_sub(1) as f32;
        if best
            .as_ref()
            .is_some_and(|(best_score, _)| *best_score > smaller_max)
        {
            break;
        }
    }
    Ok(best.map(|(_, scan)| scan).unwrap_or(Scan {
        cards: Vec::new(),
        geometry: row.source,
    }))
}

// Squad size is an upper bound. The frame decides the row: the hinted layout
// wins when every card is high, a smaller layout wins when every card in it is
// high, and a smaller layout that is a centered subset of the hint also needs
// its outer cells to be visually empty. If nothing is fully readable, keep the
// hinted count so a visible card is never dropped.
fn scan_hinted(
    catalog: &catalog::Catalog,
    image: &image::RgbImage,
    pools: &Pools,
    crop_dir: &Path,
    hint: u32,
    row: RowGeometry,
    started: Instant,
) -> std::result::Result<Scan, String> {
    let top = row.top.unwrap_or_else(|| reward::fixed_top(image.height()));
    let mut hinted = Scan {
        cards: scan_layout_luma(catalog, image, pools, crop_dir, hint, row.top)?,
        geometry: row.source,
    };
    if scan_complete(&hinted, None) {
        return Ok(hinted);
    }
    for cards in (1..hint).rev() {
        if started.elapsed() >= OCR_BUDGET {
            break;
        }
        let scan = Scan {
            cards: scan_layout_luma(catalog, image, pools, crop_dir, cards, row.top)?,
            geometry: row.source,
        };
        if !scan_complete(&scan, None) {
            continue;
        }
        if reward::is_centered_subset(hint, cards)
            && !reward::extra_cells_absent(image, hint, cards, top)
        {
            continue;
        }
        return Ok(scan);
    }
    rescue_layout(&mut hinted, catalog, pools, crop_dir, started);
    Ok(hinted)
}

fn scan_layout_luma(
    catalog: &catalog::Catalog,
    image: &image::RgbImage,
    pools: &Pools,
    crop_dir: &Path,
    cards: u32,
    detected: Option<u32>,
) -> std::result::Result<Vec<CardScan>, String> {
    let crops = reward::layout_image_with_top(image, cards, crop_dir, detected)?;
    Ok(crops
        .into_iter()
        .map(|crop| {
            let (chosen, confidence) = match_crop(catalog, &crop, pools);
            CardScan {
                crop,
                chosen,
                confidence,
            }
        })
        .collect())
}

fn rescue_layout(
    scan: &mut Scan,
    catalog: &catalog::Catalog,
    pools: &Pools,
    crop_dir: &Path,
    started: Instant,
) {
    for card in &mut scan.cards {
        if card.confidence.tier == ConfidenceTier::High || started.elapsed() >= OCR_BUDGET {
            continue;
        }
        let extras = reward::reocr_image(&card.crop, crop_dir);
        if extras.is_empty() {
            continue;
        }
        card.crop.texts.extend(extras);
        let (chosen, confidence) = match_crop(catalog, &card.crop, pools);
        card.chosen = chosen;
        card.confidence = confidence;
    }
}

fn crop_dir() -> PathBuf {
    fetch::cache_dir().join("crops")
}

fn scan_path(
    catalog: &catalog::Catalog,
    path: &Path,
    pools: &Pools,
    hint: Option<u32>,
) -> std::result::Result<Scan, String> {
    let image = image::open(path)
        .map_err(|error| format!("open {}: {error}", path.display()))?
        .to_rgb8();
    scan_image(catalog, &image, pools, &crop_dir(), hint)
}

// The early (4 s) capture may open the HUD only when a whole row is readable:
// the hinted count when every expected card is high, or a smaller centered row
// when every card in it is high. The retry accepts unknown cards with evidence.
fn scan_complete(scan: &Scan, hint: Option<u32>) -> bool {
    let expected = hint.map_or(scan.cards.len(), |count| count as usize);
    !scan.cards.is_empty()
        && scan.cards.len() == expected
        && scan
            .cards
            .iter()
            .all(|card| card.chosen.is_some() && card.confidence.tier == ConfidenceTier::High)
}

fn first_capture_readable(scan: &Scan, hint: Option<u32>) -> bool {
    scan_complete(scan, hint) || scan_complete(scan, None)
}

fn hud_capture(
    catalog: &catalog::Catalog,
    pools: &Pools,
    dir: &Path,
    hint: Option<u32>,
    db: Option<&itemdb::ItemDb>,
    strict: bool,
) -> std::result::Result<Option<u32>, String> {
    let started = Instant::now();
    let image = probe_image_frame(dir).map_err(|error| format!("capture: {error}"))?;
    let (width, height) = image.dimensions();
    let crop_dir = fetch::cache_dir().join("crops");
    let scan = scan_image(catalog, &image, pools, &crop_dir, hint)
        .map_err(|error| format!("ocr: {error}"))?;
    let card_ms = started.elapsed().as_millis();
    let source = if std::env::var_os("CORUFRAME_CAPTURE_CMD").is_some() {
        "command"
    } else {
        "portal"
    };
    log_ocr(&scan, width, height, card_ms);
    maybe_keep_frame(&image, &scan, source);
    if scan.cards.is_empty() {
        return Ok(None);
    }
    if strict && !first_capture_readable(&scan, hint) {
        eprintln!("hud: early capture incomplete; retrying");
        return Ok(None);
    }
    // Warm the exact art for the cards we are about to draw. The mission-time
    // prefetch usually has it; this covers an unknown equipped relic or a cold
    // cache, and never blocks the HUD for more than the short budget.
    if let Some(db) = db {
        let names: Vec<String> = scan
            .cards
            .iter()
            .filter_map(|card| card.chosen.as_ref().map(|chosen| chosen.name.clone()))
            .collect();
        hud::warm_images(db, &names, Duration::from_millis(500));
    }
    let session = Session::new()?.keep_if(keep_sessions());
    let counts = coruframe_core::inventory_keys::load_counts();
    let cached = price::app_quotes();
    let payload = payload_for(
        &session,
        &scan,
        width,
        height,
        source,
        &PayloadContext {
            db,
            counts: counts.as_ref(),
            cached: &cached,
        },
    )?;
    let payload_path = session.dir.join("payload.json");
    payload.write(&session.dir, &payload_path)?;
    eprintln!(
        "hud: {} card(s) in {}ms -> {}",
        scan.cards.len(),
        started.elapsed().as_millis(),
        payload_path.display()
    );
    spawn_hud_payload(&payload_path, session)
        .map_err(|error| format!("spawn hud: {error}"))
        .map(Some)
}

struct Session {
    dir: PathBuf,
    keep: bool,
}

impl Session {
    fn new() -> std::result::Result<Self, String> {
        Self::at(fetch::cache_dir().join("ocr-sessions"))
    }

    // Kept sessions are the evidence export: the card and name-strip PNGs stay
    // for the user after the HUD exits.
    fn export() -> std::result::Result<Self, String> {
        Ok(Self::at(fetch::cache_dir().join("ocr-crops"))?.keep_if(true))
    }

    fn at(parent: PathBuf) -> std::result::Result<Self, String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let dir = parent.join(format!("{stamp}-{}", std::process::id()));
        private_fs::ensure_private_dir(&dir).map_err(|error| format!("session: {error}"))?;
        Ok(Self { dir, keep: false })
    }

    fn keep_if(mut self, keep: bool) -> Self {
        self.keep = keep;
        self
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.keep {
            eprintln!("hud: kept session {}", self.dir.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

fn keep_sessions() -> bool {
    std::env::var("CORUFRAME_KEEP_OCR_SESSION")
        .map(|value| matches!(value.as_str(), "1" | "true" | "on" | "yes"))
        .unwrap_or(false)
}

fn png_bytes(image: &image::RgbImage) -> std::result::Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(image.clone())
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|error| format!("png: {error}"))?;
    Ok(bytes)
}

struct PayloadContext<'a> {
    db: Option<&'a itemdb::ItemDb>,
    counts: Option<&'a HashMap<String, u64>>,
    cached: &'a HashMap<String, u32>,
}

fn payload_for(
    session: &Session,
    scan: &Scan,
    width: u32,
    height: u32,
    source: &str,
    context: &PayloadContext,
) -> std::result::Result<RewardPayload, String> {
    let mut cards = Vec::new();
    for (index, card) in scan.cards.iter().enumerate() {
        let card_name = format!("card-{index}.png");
        let name_name = format!("name-{index}.png");
        private_fs::write_private(&session.dir.join(&card_name), &png_bytes(&card.crop.color)?)
            .map_err(|error| format!("write {card_name}: {error}"))?;
        private_fs::write_private(
            &session.dir.join(&name_name),
            &png_bytes(&card.crop.name_strip())?,
        )
        .map_err(|error| format!("write {name_name}: {error}"))?;
        // Fill the set/part/price/owned fields here, not in the HUD: the payload
        // is the handoff boundary and must be enough to draw a card.
        let mut chosen = card.chosen.clone();
        let mut owned = None;
        if let Some(chosen) = &mut chosen {
            if !chosen.untradeable {
                if let Some((parent, component)) = context
                    .db
                    .and_then(|db| itemdb::find_reward_component(db, &chosen.name))
                {
                    chosen.set = Some(parent.name.clone());
                    chosen.part = Some(component.name.clone());
                    owned = context.counts.map(|counts| {
                        counts
                            .get(&component.unique_name)
                            .copied()
                            .unwrap_or(0)
                            .min(u32::MAX as u64) as u32
                    });
                }
            }
        }
        let price = chosen
            .as_ref()
            .filter(|chosen| !chosen.untradeable)
            .and_then(|chosen| context.cached.get(&chosen.slug).copied());
        cards.push(CardPayload {
            index: index as u32,
            rect: Rect {
                left: card.crop.left,
                top: card.crop.top,
                width: card.crop.width,
                height: card.crop.height,
            },
            card_crop: Some(card_name.into()),
            name_crop: name_name.into(),
            ocr: card
                .crop
                .texts
                .iter()
                .map(|text| OcrVariant {
                    engine: "tesseract".into(),
                    psm: text.psm,
                    variant: text.variant.clone(),
                    text: text.text.clone(),
                    confidence: text.confidence,
                    error: text.error.clone(),
                })
                .collect(),
            chosen,
            confidence: card.confidence.clone(),
            price,
            owned,
        });
    }
    Ok(RewardPayload {
        session: session
            .dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        frame: FrameInfo {
            width,
            height,
            captured_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            source: source.to_string(),
            geometry: scan.geometry.as_str().to_string(),
        },
        cards,
    })
}

fn hud_log() -> Option<std::fs::File> {
    let path = fetch::cache_dir().join("hud.log");
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)
        .ok()
}

fn spawn_hud_payload(payload: &Path, session: Session) -> std::result::Result<u32, String> {
    let exe = std::env::current_exe().map_err(|error| format!("current exe: {error}"))?;
    let seconds = REWARD_WINDOW.as_secs().to_string();
    let mut command = std::process::Command::new(exe);
    command.args([
        "hud",
        "--payload",
        payload.to_string_lossy().as_ref(),
        "--seconds",
        seconds.as_str(),
    ]);
    if let Some(log) = hud_log() {
        if let Ok(err) = log.try_clone() {
            command.stdout(log).stderr(err);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn hud: {error}"))?;
    let pid = child.id();
    std::thread::spawn(move || {
        let _ = child.wait();
        drop(session);
    });
    Ok(pid)
}

fn probe_image(
    catalog: &catalog::Catalog,
    rewards: Option<&HashSet<String>>,
    image: &Path,
    stage: &mut Stage,
) -> std::result::Result<(), String> {
    let pools = Pools {
        local: None,
        global: rewards.cloned(),
    };
    let started = Instant::now();
    let scan = scan_path(catalog, image, &pools, None)?;
    stage.log(&format!(
        "ocr {}ms, {} card(s), geometry {}",
        started.elapsed().as_millis(),
        scan.cards.len(),
        scan.geometry.as_str()
    ));
    let mut matches = Vec::new();
    for (index, card) in scan.cards.iter().enumerate() {
        println!(
            "card {index} @{},{} {}x{}:",
            card.crop.left, card.crop.top, card.crop.width, card.crop.height
        );
        for text in &card.crop.texts {
            println!(
                "  {:<7} psm {:<2} {:>5.1} {:?}",
                text.variant,
                text.psm,
                text.confidence,
                text.text.trim()
            );
        }
        match (&card.chosen, card.confidence.tier) {
            (Some(chosen), tier) => {
                println!(
                    "  -> {} ({:?}, coverage {:.3}, margin {:.3}, agreement {:.2})",
                    chosen.name,
                    tier,
                    card.confidence.evidence.coverage,
                    card.confidence.evidence.margin,
                    card.confidence.evidence.variant_agreement
                );
                matches.push(catalog::Match {
                    slug: chosen.slug.clone(),
                    name: chosen.name.clone(),
                    left: card.crop.left,
                    top: card.crop.top,
                    width: card.crop.width,
                    height: card.crop.height,
                });
            }
            (None, tier) => println!("  -> unknown ({tier:?})"),
        }
    }
    stage.log(&format!("matches {}", describe(&matches)));
    if let Some((width, height)) = png_size(image) {
        stage.log(&format!("frame {width}x{height}"));
    }
    Ok(())
}

fn size_text(size: Option<(u32, u32)>) -> String {
    match size {
        Some((width, height)) => format!("{width}x{height}"),
        None => "unknown size".to_string(),
    }
}

// Append-only and flushed per line: a killed probe still leaves the last stage
// behind. The log is the source of truth; stdout stays for humans.
fn acquire_probe_lock(stage: &mut Stage) -> Result<std::fs::File> {
    let path = fetch::cache_dir().join("probe.lock");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(&path)?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if rc != 0 {
        stage.log("another probe is running; exiting");
        return Err("another probe is already running".into());
    }
    stage.log(&format!("probe lock {}", path.display()));
    Ok(file)
}

struct Stage {
    file: Option<std::fs::File>,
}

impl Stage {
    fn open(path: &Path) -> Self {
        match append_private(path) {
            Ok(file) => Self { file: Some(file) },
            Err(error) => {
                eprintln!("probe log {}: {error}", path.display());
                Self { file: None }
            }
        }
    }

    fn log(&mut self, message: &str) {
        let Some(file) = &mut self.file else {
            return;
        };
        use std::io::Write;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let _ = writeln!(
            file,
            "{}.{:03} probe {message}",
            now.as_secs(),
            now.subsec_millis()
        );
        let _ = file.flush();
    }
}

#[derive(Default)]
struct ArmState {
    until: Option<Instant>,
    last: Option<Instant>,
    first_at: Option<Instant>,
    attempts: u32,
}

impl ArmState {
    fn arm(&mut self, now: Instant) {
        self.until = Some(now + REWARD_WINDOW);
        self.last = None;
        self.first_at = None;
        self.attempts = 0;
    }

    fn arm_delayed(&mut self, now: Instant, delay: Duration) {
        self.arm(now);
        self.first_at = Some(now + delay);
    }

    fn next_capture(&mut self, now: Instant) -> bool {
        if self.until.is_none() || self.expired(now) {
            return false;
        }
        if let Some(first) = self.first_at {
            if now < first {
                return false;
            }
            self.first_at = None;
        }
        if self
            .last
            .is_none_or(|last| now.saturating_duration_since(last) >= POLL_INTERVAL)
        {
            self.last = Some(now);
            self.attempts += 1;
            true
        } else {
            false
        }
    }

    fn expired(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now >= until)
    }

    fn disarm(&mut self) {
        self.until = None;
        self.last = None;
        self.first_at = None;
        self.attempts = 0;
    }
}

fn describe(matches: &[catalog::Match]) -> String {
    matches
        .iter()
        .map(|found| {
            format!(
                "{} @{},{},{}x{}",
                found.name, found.left, found.top, found.width, found.height
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

// Export the same evidence the HUD draws, without a display or a spawn. Used to
// inspect the name strips and to add frames to the corpus.
fn ocr_crops(args: &[String]) -> Result<()> {
    let mut frame = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--frame" => frame = Some(PathBuf::from(iter.next().ok_or("--frame needs a path")?)),
            other => return Err(format!("unknown option: {other}").into()),
        }
    }
    let frame = frame.ok_or("usage: coruframe ocr-crops --frame PATH")?;
    sweep_debug_frames();
    if !ocr::available() {
        return Err("tesseract is not on PATH".into());
    }
    let catalog =
        coruframe_core::catalog::load(false).map_err(|error| format!("catalog: {error}"))?;
    let relics = relic::load(false).unwrap_or_default();
    let db = itemdb::ItemDb::load().ok();
    ocr::set_user_words(coruframe_core::ocr_dict::ensure_user_words(
        &catalog,
        db.as_ref(),
        &relics,
    ));
    let pools = Pools {
        local: None,
        global: reward_slugs_from(&catalog, &relics),
    };
    let image = image::open(&frame)
        .map_err(|error| format!("open {}: {error}", frame.display()))?
        .to_rgb8();
    let (width, height) = image.dimensions();
    let scan = scan_image(&catalog, &image, &pools, &crop_dir(), None)?;
    if scan.cards.is_empty() {
        return Err(format!("no reward cards found in {}", frame.display()).into());
    }
    let session = Session::export()?;
    let counts = coruframe_core::inventory_keys::load_counts();
    let cached = price::app_quotes();
    let payload = payload_for(
        &session,
        &scan,
        width,
        height,
        "image",
        &PayloadContext {
            db: db.as_ref(),
            counts: counts.as_ref(),
            cached: &cached,
        },
    )?;
    payload.write(&session.dir, &session.dir.join("payload.json"))?;
    for card in &payload.cards {
        let name = card
            .chosen
            .as_ref()
            .map(|chosen| chosen.name.as_str())
            .unwrap_or("unknown");
        let tier = format!("{:?}", card.confidence.tier).to_lowercase();
        println!(
            "{} {} [{}] {}",
            card.index,
            name,
            tier,
            session.dir.join(&card.name_crop).display()
        );
    }
    println!("crops: {}", session.dir.display());
    drop(session);
    Ok(())
}

// The frame size is in the PNG header; read it without decoding the image.
// the dimensions straight from the PNG header instead of pulling in an image crate.
fn png_size(path: &Path) -> Option<(u32, u32)> {
    capture::png_size(path)
}

// A fissure card can only be a relic reward; the market catalog also contains
// mods, arcanes and sets that are pure noise for matching.
fn reward_slugs_from(
    catalog: &catalog::Catalog,
    relics: &[relic::Entry],
) -> Option<HashSet<String>> {
    let slugs: HashSet<String> = relics
        .iter()
        .flat_map(|entry| entry.rewards.iter())
        .filter_map(|reward| catalog.resolve(&reward.item_name))
        .map(|item| item.slug.clone())
        .collect();
    (!slugs.is_empty()).then_some(slugs)
}

// The equipped relic from the log narrows the pool before any image work. The
// reward screen can only show its outcomes, so a vague OCR can resolve inside a
// handful of names instead of all 603 relic rewards.
fn local_relic_pool(
    catalog: &catalog::Catalog,
    relics: &[relic::Entry],
    relic_name: &str,
) -> Option<HashSet<String>> {
    let slugs: HashSet<String> = relic::find(relics, relic_name)
        .into_iter()
        .flat_map(|entry| entry.rewards.iter())
        .filter_map(|reward| catalog.resolve(&reward.item_name))
        .map(|item| item.slug.clone())
        .collect();
    (!slugs.is_empty()).then_some(slugs)
}

// Warm the images and order cache while the mission is still running. The HUD
// then starts from cache instead of spending its 20 s window on the network.
fn prefetch_outcomes(
    catalog: &catalog::Catalog,
    relics: &[relic::Entry],
    relic_name: Option<&str>,
    seen: &mut HashSet<String>,
) {
    let Some(relic_name) = relic_name else {
        return;
    };
    let mut names: Vec<String> = relic::find(relics, relic_name)
        .into_iter()
        .flat_map(|entry| entry.rewards.iter())
        .map(|reward| reward.item_name.clone())
        .filter(|name| !seen.contains(name))
        .collect();
    names.sort();
    names.dedup();
    names.truncate(12);
    if names.is_empty() {
        return;
    }
    for name in &names {
        seen.insert(name.clone());
    }
    let catalog = catalog.clone();
    std::thread::spawn(move || {
        let db = itemdb::ItemDb::load().ok();
        if let Some(db) = &db {
            hud::warm_images(db, &names, Duration::from_secs(4));
        }
        for name in names {
            if let Some(item) = catalog.resolve(&name) {
                let _ = price::quote_within(item, Duration::from_secs(3));
            }
            std::thread::sleep(Duration::from_millis(350));
        }
    });
}

fn log_ocr(scan: &Scan, width: u32, height: u32, card_ms: u128) {
    use std::io::Write;
    let cards: Vec<serde_json::Value> = scan
        .cards
        .iter()
        .enumerate()
        .map(|(index, card)| {
            serde_json::json!({
                "index": index,
                "rect": [card.crop.left, card.crop.top, card.crop.width, card.crop.height],
                "ocr": card.crop.texts.iter().map(|text| serde_json::json!({
                    "variant": text.variant,
                    "psm": text.psm,
                    "text": text.text.trim(),
                    "confidence": text.confidence,
                    "error": text.error,
                })).collect::<Vec<_>>(),
                "chosen": card.chosen,
                "confidence": card.confidence,
            })
        })
        .collect();
    let line = serde_json::json!({
        "ts_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        "frame": {"width": width, "height": height, "geometry": scan.geometry.as_str()},
        "cards": cards,
        "timing_ms": card_ms,
    });
    let path = fetch::cache_dir().join("ocr.log");
    match append_private(&path) {
        Ok(mut file) => {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
        Err(error) => eprintln!("ocr.log {}: {error}", path.display()),
    }
}

fn keep_frames_mode() -> Option<String> {
    std::env::var("CORUFRAME_KEEP_OCR_FRAMES")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| value == "all" || value == "miss")
}

// Opt-in, local, 0600 full-frame capture for the corpus. Production never
// writes a frame: the env var has to be set explicitly.
fn maybe_keep_frame(image: &image::RgbImage, scan: &Scan, source: &str) {
    let Some(mode) = keep_frames_mode() else {
        return;
    };
    let missed = scan.cards.iter().any(|card| card.chosen.is_none());
    if mode == "miss" && !missed {
        return;
    }
    let dir = fetch::cache_dir().join("ocr-frames");
    if private_fs::ensure_private_dir(&dir).is_err() {
        return;
    }
    let (width, height) = image.dimensions();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let stem = format!("{stamp}-{width}x{height}");
    let frame_path = dir.join(format!("{stem}.png"));
    let bytes = match png_bytes(image) {
        Ok(bytes) => bytes,
        Err(error) => {
            eprintln!("frame capture: {error}");
            return;
        }
    };
    if let Err(error) = private_fs::write_private(&frame_path, &bytes) {
        eprintln!("frame capture {}: {error}", frame_path.display());
        return;
    }
    let meta = serde_json::json!({
        "file": format!("{stem}.png"),
        "width": width,
        "height": height,
        "captured_unix_ms": stamp,
        "source": source,
        "geometry": scan.geometry.as_str(),
        "cards": scan.cards.iter().enumerate().map(|(index, card)| serde_json::json!({
            "index": index,
            "rect": [card.crop.left, card.crop.top, card.crop.width, card.crop.height],
            "chosen": card.chosen,
            "tier": card.confidence.tier,
            "ocr": card.crop.texts.iter().map(|text| text.text.trim()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    });
    let meta_path = dir.join(format!("{stem}.json"));
    if let Err(error) = private_fs::write_private(&meta_path, meta.to_string().as_bytes()) {
        eprintln!("frame meta {}: {error}", meta_path.display());
    }
    sweep_debug_frames();
}

fn sweep_sessions() {
    sweep_dir(&fetch::cache_dir().join("ocr-sessions"), SESSION_TTL, true);
}

fn sweep_debug_frames() {
    sweep_dir(
        &fetch::cache_dir().join("ocr-frames"),
        DEBUG_FRAME_TTL,
        false,
    );
    // `ocr-crops` exports are kept for inspection but not forever.
    sweep_dir(&fetch::cache_dir().join("ocr-crops"), DEBUG_FRAME_TTL, true);
}

fn sweep_dir(dir: &Path, max_age: Duration, directories: bool) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let old = entry
            .metadata()
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if !old {
            continue;
        }
        let _ = if directories {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
}

fn rotate_log(path: &Path) {
    let too_big = std::fs::metadata(path).is_ok_and(|meta| meta.len() > OCR_LOG_MAX_BYTES);
    if too_big {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
}

fn count_lines(path: &Path) -> u64 {
    std::fs::read(path)
        .map(|bytes| bytes.iter().filter(|byte| **byte == b'\n').count() as u64)
        .unwrap_or(0)
}

// Frame names are timestamp-ordered, so a lexicographic sort is chronological.
// Only files this command writes are pruned: --out may point at a shared folder.
fn prune_frames(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut frames: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_frame_name(path))
        .collect();
    frames.sort();
    let excess = frames.len().saturating_sub(KEEP_FRAMES);
    for path in frames.into_iter().take(excess) {
        let _ = std::fs::remove_file(path);
    }
}

fn is_frame_name(path: &Path) -> bool {
    private_fs::is_frame_name(path)
}

fn print_ocr(text: &str) {
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(60)
    {
        println!("  {line}");
    }
}

fn install_desktop(args: &[String]) -> Result<()> {
    let mut dir = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .ok_or("no HOME")?
        .join("applications");
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--dir" => dir = iter.next().ok_or("--dir needs a path")?.into(),
            other => return Err(format!("unknown option: {other}").into()),
        }
    }
    let exe = std::env::current_exe()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("coruframe.desktop");
    std::fs::write(&path, desktop_entry(&exe)?)?;
    println!("{}", path.display());
    Ok(())
}

// KWin resolves the D-Bus caller's /proc/PID/exe and allows ScreenShot2 only when
// an installed desktop file names that binary in Exec=. A newline would inject
// another key, so control characters are refused instead of escaped.
fn desktop_entry(exe: &Path) -> std::result::Result<String, String> {
    let raw = exe.display().to_string();
    if raw.chars().any(|character| character.is_control()) {
        return Err("the executable path contains control characters".into());
    }
    let exe = raw.replace('\\', "\\\\").replace('"', "\\\"");
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName=CoruFrame\nExec=\"{exe}\"\nX-DBUS-StartupType=Unique\nX-DBUS-ServiceName=xyz.coruscnium.coruframe\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n"
    ))
}

fn price_cmd(args: &[String]) -> Result<()> {
    let query = query_of(args);
    if query.is_empty() {
        return Err("usage: coruframe price <item>".into());
    }
    let catalog = catalog::load(false)?;
    let item = catalog
        .resolve(&query)
        .ok_or_else(|| format!("unknown item: {query}"))?;
    match price::quote(item)? {
        Some((platinum, online)) => println!(
            "{} ({}): {platinum}p, {online} online seller(s)",
            item.name(),
            item.slug
        ),
        None => println!("{} ({}): no online sell orders", item.name(), item.slug),
    }
    Ok(())
}

fn relic_cmd(args: &[String]) -> Result<()> {
    let with_price = args.iter().any(|arg| arg == "--price");
    let query = query_of(args);
    if query.is_empty() {
        return Err("usage: coruframe relic <tier> <name> [--price]".into());
    }
    let catalog = catalog::load(false)?;
    let entries = relic::load(false)?;
    let found = relic::find(&entries, &query);
    if found.is_empty() {
        return Err(format!("no relic matches {query:?}").into());
    }

    let mut grouped: BTreeMap<(String, String), Vec<&relic::Entry>> = BTreeMap::new();
    for entry in found {
        grouped
            .entry((entry.tier.clone(), entry.name.clone()))
            .or_default()
            .push(entry);
    }

    let mut prices: BTreeMap<String, Option<(u64, usize)>> = BTreeMap::new();
    let mut price_error = false;
    if with_price {
        let mut names: Vec<&str> = grouped
            .values()
            .flatten()
            .flat_map(|entry| entry.rewards.iter().map(|reward| reward.item_name.as_str()))
            .collect();
        names.sort_unstable();
        names.dedup();
        for name in names {
            let Some(item) = catalog.resolve(name) else {
                continue; // untradable reward, worth no platinum
            };
            match price::quote(item) {
                Ok(quote) => {
                    prices.insert(name.to_string(), quote);
                }
                Err(_) => price_error = true,
            }
        }
    }

    for ((tier, name), mut rows) in grouped {
        rows.sort_by_key(|entry| relic::state_rank(&entry.state));
        let label = if name.is_empty() {
            tier.clone()
        } else {
            format!("{tier} {name}")
        };
        let vaulted = catalog
            .resolve(&format!("{tier} {name} Relic"))
            .and_then(|item| item.vaulted)
            .unwrap_or(false);
        println!("{label}{}", if vaulted { " (vaulted)" } else { "" });
        for entry in rows {
            let ducats = relic::expected_ducats(entry, &catalog);
            let platinum = if with_price {
                if price_error {
                    ", platinum unavailable".to_string()
                } else {
                    match relic::expected_plat(entry, &prices) {
                        Some(value) => format!(", {value:.1}p expected"),
                        None => ", platinum unavailable".to_string(),
                    }
                }
            } else {
                String::new()
            };
            println!("  {:<12} {ducats:.1} ducats{platinum}", entry.state);
            for reward in &entry.rewards {
                let ducats = catalog
                    .resolve(&reward.item_name)
                    .and_then(|item| item.ducats)
                    .map_or("—".to_string(), |value| format!("{value}d"));
                println!(
                    "    {:<40} {:>6.2}%  {:>4}  {}",
                    reward.item_name, reward.chance, ducats, reward.rarity
                );
            }
        }
    }
    Ok(())
}

fn catalog_cmd(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("update") => {
            catalog::load(true)?;
            relic::load(true)?;
            fetch::fetch(
                catalog::WORLDSTATE_URL,
                "worldstate.json",
                Duration::from_secs(300),
            )?;
            println!("catalog refreshed in {}", fetch::cache_dir().display());
        }
        _ => return Err("usage: coruframe catalog update".into()),
    }
    Ok(())
}

fn itemdb_cmd(args: &[String]) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("update") => {
            let db = itemdb::ItemDb::build(true)?;
            for (category, count) in &db.counts {
                println!("{category:<18} {count:>5}");
            }
            println!(
                "{} items in {}",
                db.all().len(),
                itemdb::ItemDb::path().display()
            );
        }
        Some("audit") => {
            let path = args
                .iter()
                .position(|arg| arg == "--snapshot")
                .and_then(|index| args.get(index + 1))
                .map(PathBuf::from)
                .unwrap_or_else(|| fetch::cache_dir().join("inventory-raw.json"));
            itemdb_audit(&path)?;
        }
        _ => return Err("usage: coruframe itemdb update | itemdb audit [--snapshot PATH]".into()),
    }
    Ok(())
}

fn itemdb_audit(path: &Path) -> Result<()> {
    let db = itemdb::ItemDb::load()?;
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let value: serde_json::Value = serde_json::from_str(&text)?;
    let arrays = [
        "MiscItems",
        "Suits",
        "LongGuns",
        "Pistols",
        "Melee",
        "SpaceSuits",
        "RawUpgrades",
        "Upgrades",
    ];
    let mut entries = 0usize;
    let mut resolved = 0usize;
    let mut unresolved: Vec<(String, String)> = Vec::new();
    for key in arrays {
        let Some(items) = value.get(key).and_then(serde_json::Value::as_array) else {
            continue;
        };
        for item in items {
            let Some(item_path) = item.get("ItemType").and_then(serde_json::Value::as_str) else {
                continue;
            };
            entries += 1;
            if db.resolve(item_path).is_none() {
                unresolved.push((key.to_string(), item_path.to_string()));
            } else {
                resolved += 1;
            }
        }
    }
    unresolved.sort();
    unresolved.dedup();
    println!(
        "itemdb: {} items, snapshot {}",
        db.all().len(),
        path.display()
    );
    println!(
        "resolved {resolved}/{entries} entries ({} unique unresolved paths)",
        unresolved.len()
    );
    for (array, item_path) in &unresolved {
        println!("  {array:<12} {item_path}");
    }
    match value.get("XPInfo").and_then(serde_json::Value::as_array) {
        Some(xp) => {
            let keys = xp
                .first()
                .and_then(serde_json::Value::as_object)
                .map(|entry| entry.keys().cloned().collect::<Vec<_>>().join(","))
                .unwrap_or_default();
            let mastery = Mastery::from_profile(&value, &db);
            println!(
                "XPInfo: captured, {} entries, first entry keys: {keys}",
                xp.len()
            );
            println!(
                "mastery: {} mastered / {} equipment in XPInfo",
                mastery.mastered.len(),
                mastery.xp.len()
            );
        }
        None => println!("XPInfo: not in the snapshot (mastery stays ownership-only)"),
    }
    Ok(())
}

fn query_of(args: &[String]) -> String {
    args.iter()
        .filter(|arg| !arg.starts_with("--"))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ")
}

fn log_path(explicit: Option<PathBuf>) -> Result<PathBuf> {
    match explicit {
        Some(path) => Ok(path),
        None => logpath::find().ok_or_else(|| "EE.log not found; pass --log".into()),
    }
}

fn open_ledger(db: &Option<PathBuf>) -> Result<Ledger> {
    let path = db.clone().unwrap_or_else(default_db);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(Ledger::open(&path)?)
}

fn default_db() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("coruframe/ledger.db")
}

// The line offset counts newline-terminated lines so a trailing partial is re-read
// from its start rather than lost; `Follower` starts at that same boundary.
fn read_start(path: &Path) -> (Tracker, u64) {
    let text = std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let lines = text.matches('\n').count() as u64;
    (Tracker::new(session::from_text(&text)), lines)
}

fn emit(event: &Event, json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string(event).expect("events serialize")
        );
        return;
    }
    match event {
        Event::Login { ts_ms, user, .. } => println!("{} login {user}", stamp(*ts_ms)),
        Event::Loadout { ts_ms, loadout, .. } => println!(
            "{} loadout suits={} weapons={} mods={} arcanes={} gear={} other={}",
            stamp(*ts_ms),
            loadout.suits.len(),
            loadout.weapons.len(),
            loadout.mods.len(),
            loadout.arcanes.len(),
            loadout.gear.len(),
            loadout.other.len()
        ),
        Event::MissionEnd {
            ts_ms,
            success,
            reward_tiers,
            credit_bonus,
            ..
        } => println!(
            "{} mission_end success={success} tiers={reward_tiers:?} credits={credit_bonus}",
            stamp(*ts_ms)
        ),
        Event::RelicExtraction { ts_ms, .. } => println!("{} relic_extraction", stamp(*ts_ms)),
        Event::RelicEquipped {
            ts_ms,
            relic,
            state,
            ..
        } => println!("{} relic_equipped {relic} [{state}]", stamp(*ts_ms)),
        Event::RelicReward {
            ts_ms,
            player_id,
            game_ref,
            ..
        } => println!(
            "{} relic_reward player={player_id} item={}",
            stamp(*ts_ms),
            game_ref.rsplit('/').next().unwrap_or(game_ref)
        ),
        Event::SquadSize { ts_ms, count, .. } => {
            println!("{} squad_size {count}", stamp(*ts_ms))
        }
    }
}

fn stamp(ts_ms: u64) -> String {
    format!("{}.{:03}", ts_ms / 1000, ts_ms % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_equipped_relic_narrows_the_candidate_pool() {
        let catalog = catalog::Catalog::from_json(
            r#"{"data":[
              {"slug":"braton_prime_barrel","gameRef":"","tags":[],"i18n":{"en":{"name":"Braton Prime Barrel"}}},
              {"slug":"lex_prime_receiver","gameRef":"","tags":[],"i18n":{"en":{"name":"Lex Prime Receiver"}}}
            ]}"#,
        )
        .unwrap();
        let relics = vec![relic::Entry {
            tier: "Lith".into(),
            name: "G3".into(),
            state: "RADIANT".into(),
            rewards: vec![relic::Reward {
                item_name: "Lex Prime Receiver".into(),
                rarity: "Uncommon".into(),
                chance: 20.0,
            }],
        }];
        let pool = local_relic_pool(&catalog, &relics, "Lith G3").unwrap();
        assert_eq!(pool.len(), 1);
        let found = catalog
            .best_reward_match_in("Lex Prime Receiver", Some(&pool), MatchSource::Local)
            .unwrap();
        assert_eq!(found.item.slug, "lex_prime_receiver");
        assert!(catalog
            .best_reward_match_in("Braton Prime Barrel", Some(&pool), MatchSource::Local)
            .is_none());
    }

    #[test]
    fn forma_needs_a_majority_of_the_variants() {
        let catalog = catalog::Catalog::from_json(r#"{"data":[]}"#).unwrap();
        let crop = |texts: &[&str]| reward::Crop {
            left: 0,
            top: 0,
            width: 10,
            height: 10,
            texts: texts
                .iter()
                .map(|text| reward::OcrText {
                    variant: "test".into(),
                    psm: 6,
                    text: (*text).into(),
                    confidence: 80.0,
                    error: None,
                })
                .collect(),
            color: image::RgbImage::new(10, 10),
        };
        let (chosen, confidence) = match_crop(
            &catalog,
            &crop(&["Forma Blueprint", "FormabBlueprint"]),
            &Pools::default(),
        );
        assert!(chosen.unwrap().untradeable);
        assert_eq!(confidence.tier, ConfidenceTier::High);
        let (chosen, _) = match_crop(
            &catalog,
            &crop(&["Forma Blueprint", "noise", "noise", "noise"]),
            &Pools::default(),
        );
        assert!(chosen.is_none(), "one vote out of four is not Forma");
    }

    #[test]
    fn the_early_capture_needs_every_expected_card_high() {
        let card = |high: bool| CardScan {
            crop: reward::Crop {
                left: 0,
                top: 0,
                width: 10,
                height: 10,
                texts: Vec::new(),
                color: image::RgbImage::new(10, 10),
            },
            chosen: high.then(|| MatchedItem {
                slug: "x".into(),
                name: "X".into(),
                set: None,
                part: None,
                untradeable: false,
            }),
            confidence: Confidence {
                tier: if high {
                    ConfidenceTier::High
                } else {
                    ConfidenceTier::Low
                },
                score: f32::from(high),
                evidence: MatchEvidence::default(),
            },
        };
        let scan = |highs: &[bool]| Scan {
            cards: highs.iter().map(|high| card(*high)).collect(),
            geometry: reward::GeometrySource::Fixed,
        };
        assert!(scan_complete(&scan(&[true, true]), Some(2)));
        assert!(!scan_complete(&scan(&[true, true]), Some(4)));
        assert!(!scan_complete(&scan(&[true, false]), Some(2)));
        assert!(scan_complete(&scan(&[true]), None));
        assert!(!scan_complete(&scan(&[false]), None));
        assert!(!scan_complete(&scan(&[]), None));
        assert!(first_capture_readable(&scan(&[true, true, true]), Some(4)));
        assert!(!first_capture_readable(
            &scan(&[true, true, false]),
            Some(4)
        ));
        assert!(first_capture_readable(
            &scan(&[true, true, true, true]),
            Some(4)
        ));
    }

    #[test]
    fn a_payload_keeps_unknown_cards_and_the_session_cleans_up() {
        let evidence = MatchEvidence {
            coverage: 1.0,
            anchor_len: 6,
            margin: 0.5,
            unique: true,
            variant_agreement: 1.0,
            source: MatchSource::Global,
            exact: true,
        };
        let card = |index: u32, text: &str, chosen: Option<MatchedItem>| CardScan {
            crop: reward::Crop {
                left: index * 10,
                top: 0,
                width: 10,
                height: 10,
                texts: vec![reward::OcrText {
                    variant: "luma".into(),
                    psm: 6,
                    text: text.into(),
                    confidence: 80.0,
                    error: None,
                }],
                color: image::RgbImage::from_pixel(10, 10, image::Rgb([20, 20, 20])),
            },
            chosen: chosen.clone(),
            confidence: if chosen.is_some() {
                Confidence {
                    tier: ConfidenceTier::High,
                    score: 1.0,
                    evidence: evidence.clone(),
                }
            } else {
                Confidence::default()
            },
        };
        let scan = Scan {
            cards: vec![
                card(
                    0,
                    "Braton Prime Barrel",
                    Some(MatchedItem {
                        slug: "braton_prime_barrel".into(),
                        name: "Braton Prime Barrel".into(),
                        set: None,
                        part: None,
                        untradeable: false,
                    }),
                ),
                card(1, "???", None),
            ],
            geometry: reward::GeometrySource::Fixed,
        };
        let session = Session::new().unwrap();
        let dir = session.dir.clone();
        let empty = HashMap::new();
        let payload = payload_for(
            &session,
            &scan,
            3440,
            1440,
            "test",
            &PayloadContext {
                db: None,
                counts: None,
                cached: &empty,
            },
        )
        .unwrap();
        let path = dir.join("payload.json");
        payload.write(&dir, &path).unwrap();
        let loaded = RewardPayload::load(&path).unwrap();
        assert_eq!(loaded.cards.len(), 2);
        assert_eq!(
            loaded.cards[0].chosen.as_ref().unwrap().name,
            "Braton Prime Barrel"
        );
        assert_eq!(loaded.cards[1].confidence.tier, ConfidenceTier::Low);
        let crop = RewardPayload::resolve_asset(&dir, loaded.cards[0].card_crop.as_ref().unwrap())
            .unwrap();
        assert!(crop.is_file());
        drop(session);
        assert!(!dir.exists(), "the session directory must be removed");
    }

    #[test]
    fn desktop_entry_names_the_calling_binary() {
        let entry = desktop_entry(Path::new("/opt/CoruFrame/target/debug/coruframe")).unwrap();
        assert!(entry.contains("Exec=\"/opt/CoruFrame/target/debug/coruframe\""));
        assert!(entry.contains("X-DBUS-ServiceName=xyz.coruscnium.coruframe"));
        assert!(entry.contains("X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2"));
    }

    #[test]
    fn desktop_entry_escapes_backslashes_and_rejects_control_characters() {
        let entry = desktop_entry(Path::new("/tmp/a\\b/coruframe")).unwrap();
        assert!(entry.contains(r#"Exec="/tmp/a\\b/coruframe""#), "{entry}");
        assert!(desktop_entry(Path::new("/tmp/a\nb/coruframe")).is_err());
    }

    #[test]
    fn reads_png_dimensions() {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&3440u32.to_be_bytes());
        bytes.extend_from_slice(&1440u32.to_be_bytes());
        let path = std::env::temp_dir().join("coruframe-png-size-test.png");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(png_size(&path), Some((3440, 1440)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn arm_window_is_due_every_second_and_expires() {
        let mut arm = ArmState::default();
        let start = Instant::now();
        assert!(!arm.next_capture(start));
        arm.arm(start);
        assert!(arm.next_capture(start));
        assert!(!arm.next_capture(start + Duration::from_millis(500)));
        assert!(arm.next_capture(start + POLL_INTERVAL));
        assert!(!arm.expired(start + REWARD_WINDOW - Duration::from_secs(1)));
        assert!(arm.expired(start + REWARD_WINDOW));
        assert!(!arm.next_capture(start + REWARD_WINDOW));
        arm.disarm();
        assert!(!arm.next_capture(start + Duration::from_secs(2)));
        arm.arm(start);
        arm.disarm();
        assert!(!arm.next_capture(start));
    }

    #[test]
    fn prune_keeps_newest_frames_and_spares_other_files() {
        let dir = std::env::temp_dir().join(format!("coruframe-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for index in 0..12 {
            std::fs::write(dir.join(format!("17927574{index:02}.000-{index}.png")), b"").unwrap();
        }
        std::fs::write(dir.join("Screenshot_20261023.png"), b"").unwrap();
        std::fs::write(dir.join("notes.txt"), b"").unwrap();

        prune_frames(&dir);

        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let frames: Vec<&String> = names
            .iter()
            .filter(|name| is_frame_name(Path::new(name)))
            .collect();
        assert_eq!(frames.len(), 10);
        assert!(names.contains(&"Screenshot_20261023.png".to_string()));
        assert!(names.contains(&"notes.txt".to_string()));
        assert!(!names.contains(&"1792757400.000-0.png".to_string()));
        assert!(names.contains(&"1792757411.000-11.png".to_string()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn frame_name_predicate_is_strict() {
        assert!(is_frame_name(Path::new("1792757472.339-0.png")));
        assert!(is_frame_name(Path::new("/tmp/1792757472.339-12.png")));
        assert!(!is_frame_name(Path::new("Screenshot_20261023.png")));
        assert!(!is_frame_name(Path::new("1792757472.33-0.png")));
        assert!(!is_frame_name(Path::new("1792757472.339.png")));
        assert!(!is_frame_name(Path::new("1792757472.339-0.jpg")));
    }
}
