use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ashpd::desktop::{
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType},
    PersistMode,
};
use coruframe_core::capabilities;
use coruframe_core::capture_source::{CapturedFrameInfo, SelectedStream, Selection};
use coruframe_core::private_fs;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SESSION_TIMEOUT: Duration = Duration::from_secs(10);
const SELECT_TIMEOUT: Duration = Duration::from_secs(10);
// The portal dialog is user-paced; this is the plan's budget for it.
const DIALOG_TIMEOUT: Duration = Duration::from_secs(120);
const OPEN_TIMEOUT: Duration = Duration::from_secs(10);
const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Options {
    json: bool,
    restore: bool,
    capture: bool,
    frame: Option<PathBuf>,
}

pub fn choose(args: &[String]) -> Result<()> {
    let options = Options::parse(args)?;
    if !options.json {
        eprintln!(
            "{}",
            if options.restore {
                "portal: restoring the stored grant (the dialog appears only if it expired)"
            } else {
                "portal: waiting for the system dialog (up to 120 s)"
            }
        );
    }
    let (selection, _) = runtime().block_on(request_selection(&options))?;
    if options.json {
        println!("{}", serde_json::to_string_pretty(&selection)?);
    } else {
        print_human(&selection);
    }
    Ok(())
}

// The probe calls this at startup: validate and rotate an existing grant so a
// stale token prompts here, before a mission, instead of at the reward screen.
pub fn ensure_token() -> Result<()> {
    if capabilities::capture_token_path().is_file() {
        eprintln!("portal: checking the stored grant");
        let options = Options {
            restore: true,
            capture: true,
            ..Options::default()
        };
        match runtime().block_on(request_selection(&options)) {
            Ok(_) => {
                eprintln!("portal: stored grant ready");
                return Ok(());
            }
            Err(error) => {
                eprintln!("portal: stored grant was refused ({error}); asking again");
            }
        }
    } else {
        eprintln!("portal: no stored grant; asking once");
    }
    let options = Options {
        capture: true,
        ..Options::default()
    };
    let _ = runtime().block_on(request_selection(&options))?;
    Ok(())
}

// One reward window: restore the grant, open the stream, copy one frame, store
// the rotated token, close. No dialog when the grant is valid.
pub fn capture_frame() -> Result<crate::stream::CapturedFrame> {
    if !capabilities::capture_token_path().is_file() {
        return Err("no stored grant; choose a capture source first".into());
    }
    let options = Options {
        restore: true,
        capture: true,
        ..Options::default()
    };
    let (_, frame) = runtime().block_on(request_selection(&options))?;
    frame.ok_or_else(|| "portal returned no frame".into())
}

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> &'static tokio::runtime::Runtime {
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("tokio runtime")
    })
}

impl Options {
    fn parse(args: &[String]) -> Result<Self> {
        let mut options = Options::default();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--json" => options.json = true,
                "--restore" => options.restore = true,
                "--frame" => {
                    options.frame = Some(args.next().ok_or("--frame needs a path")?.into());
                    options.capture = true;
                }
                other => return Err(format!("unknown option: {other}").into()),
            }
        }
        Ok(options)
    }
}

// A fresh grant is forced unless --restore is given: no restore token is sent,
// so the portal has to ask even when a permission already exists. Cancelling
// leaves the old token alone.
async fn request_selection(
    options: &Options,
) -> Result<(Selection, Option<crate::stream::CapturedFrame>)> {
    let token = if options.restore {
        let text = std::fs::read_to_string(capabilities::capture_token_path())
            .map_err(|_| "no stored grant; run without --restore to choose a source")?;
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("stored grant is empty; run without --restore".into());
        }
        Some(text)
    } else {
        None
    };
    let screencast = tokio::time::timeout(SESSION_TIMEOUT, Screencast::new())
        .await
        .map_err(|_| "portal: no answer within 10 s; is xdg-desktop-portal running?")?
        .map_err(portal_error)?;
    if screencast.version() < 4 {
        return Err(format!(
            "portal: ScreenCast version {} cannot persist a grant (needs 4 or newer)",
            screencast.version()
        )
        .into());
    }
    let session = tokio::time::timeout(
        SESSION_TIMEOUT,
        screencast.create_session(Default::default()),
    )
    .await
    .map_err(|_| "portal: CreateSession timed out")?
    .map_err(portal_error)?;
    let select = tokio::time::timeout(
        SELECT_TIMEOUT,
        screencast.select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_sources(SourceType::Monitor | SourceType::Window)
                .set_multiple(false)
                .set_cursor_mode(CursorMode::Hidden)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(token.as_deref()),
        ),
    )
    .await
    .map_err(|_| "portal: SelectSources timed out")?
    .map_err(portal_error)?;
    select.response().map_err(portal_error)?;
    let started = Instant::now();
    let request = tokio::time::timeout(
        DIALOG_TIMEOUT,
        screencast.start(&session, None, Default::default()),
    )
    .await
    .map_err(|_| "portal: Start timed out after 120 s")?
    .map_err(portal_error)?;
    let streams = request.response().map_err(portal_error)?;
    let start_ms = started.elapsed().as_millis() as u64;

    let mut selected = Vec::new();
    for stream in streams.streams() {
        let size = stream.size();
        selected.push(SelectedStream {
            source: source_name(stream.source_type()).to_string(),
            width: size.map(|(width, _)| width),
            height: size.map(|(_, height)| height),
            node_id: stream.pipe_wire_node_id(),
        });
    }

    // Rotate the token before touching the stream: if the frame copy fails,
    // the next capture still has a fresh token instead of the consumed one.
    let token_stored = match streams.restore_token() {
        Some(token) => {
            private_fs::write_private(&capabilities::capture_token_path(), token.as_bytes())?;
            true
        }
        None => false,
    };
    let capture_result: Result<(
        Option<crate::stream::CapturedFrame>,
        Option<CapturedFrameInfo>,
    )> = if options.capture {
        async {
            let node = selected.first().ok_or("portal returned no stream")?.node_id;
            let fd = tokio::time::timeout(
                OPEN_TIMEOUT,
                screencast.open_pipe_wire_remote(&session, Default::default()),
            )
            .await
            .map_err(|_| "portal: OpenPipeWireRemote timed out")?
            .map_err(portal_error)?;
            let captured_at = Instant::now();
            let captured = crate::stream::capture_one(fd, node)?;
            let info = match &options.frame {
                Some(path) => {
                    captured.save(path)?;
                    Some(CapturedFrameInfo {
                        path: path.display().to_string(),
                        width: captured.width,
                        height: captured.height,
                        stride: captured.stride,
                        format: captured.format.clone(),
                        buffer: captured.buffer.clone(),
                        capture_ms: captured_at.elapsed().as_millis() as u64,
                    })
                }
                None => None,
            };
            Ok::<_, Box<dyn std::error::Error>>((Some(captured), info))
        }
        .await
    } else {
        Ok((None, None))
    };
    // Close even when the stream failed; a live session can block the next Start.
    let _ = tokio::time::timeout(CLOSE_TIMEOUT, session.close()).await;
    let (capture, frame_info) = capture_result?;
    Ok((
        Selection {
            streams: selected,
            token_stored,
            restored: token.is_some(),
            start_ms,
            frame: frame_info,
        },
        capture,
    ))
}

fn print_human(selection: &Selection) {
    for stream in &selection.streams {
        let size = match (stream.width, stream.height) {
            (Some(width), Some(height)) => format!("{width}x{height}"),
            _ => "size unknown".to_string(),
        };
        println!("source: {} {size} (node {})", stream.source, stream.node_id);
    }
    println!(
        "start: {} ms ({})",
        selection.start_ms,
        if selection.restored {
            "restored"
        } else {
            "new grant"
        }
    );
    println!(
        "restore token: {}",
        if selection.token_stored {
            "stored (0600)"
        } else {
            "not offered by the portal"
        }
    );
    if let Some(frame) = &selection.frame {
        println!(
            "frame: {} {}x{} stride {} {} {} ({} ms)",
            frame.path,
            frame.width,
            frame.height,
            frame.stride,
            frame.format,
            frame.buffer,
            frame.capture_ms
        );
    }
}

fn portal_error(error: ashpd::Error) -> String {
    match error {
        ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled) => {
            "portal: cancelled; the stored grant was kept".to_string()
        }
        ashpd::Error::PortalNotFound(interface) => {
            format!("portal: {interface} is not available")
        }
        other => format!("portal: {other}"),
    }
}

fn source_name(source: Option<SourceType>) -> &'static str {
    match source {
        Some(SourceType::Monitor) => "monitor",
        Some(SourceType::Window) => "window",
        Some(SourceType::Virtual) => "virtual",
        None => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_each_portal_source() {
        assert_eq!(source_name(Some(SourceType::Monitor)), "monitor");
        assert_eq!(source_name(Some(SourceType::Window)), "window");
        assert_eq!(source_name(None), "unknown");
    }

    #[test]
    fn parses_the_spike_flags() {
        let options = Options::parse(&[
            "--json".into(),
            "--restore".into(),
            "--frame".into(),
            "/tmp/f.png".into(),
        ])
        .unwrap();
        assert!(options.json);
        assert!(options.restore);
        assert!(options.capture);
        assert_eq!(
            options.frame.as_deref(),
            Some(std::path::Path::new("/tmp/f.png"))
        );
    }
}
