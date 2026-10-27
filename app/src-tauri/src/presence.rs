use std::io;
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};
use tungstenite::client::IntoClientRequest;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{connect, Message, WebSocket};

// warframe.market accepts this socket only from an allowed Origin. The webview
// cannot send one, so the connection lives here where no Origin is sent.
const SOCKET_URL: &str = "wss://ws.warframe.market/socket";
const POLL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenceStatus {
    Online,
    InGame,
    Invisible,
    // Online while the game is closed, in-game while it runs.
    Auto,
}

impl PresenceStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "online" => Some(Self::Online),
            "ingame" => Some(Self::InGame),
            "invisible" => Some(Self::Invisible),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    fn wire(self, game_running: bool) -> &'static str {
        match self {
            Self::Online => "online",
            Self::InGame => "ingame",
            Self::Invisible => "invisible",
            Self::Auto if game_running => "ingame",
            Self::Auto => "online",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PresenceDto {
    pub connected: bool,
    pub error: Option<String>,
    // The status warframe.market actually has, once Auto has picked one.
    pub effective: Option<String>,
}

struct Command {
    status: Option<PresenceStatus>,
    reconnect: bool,
}

pub struct PresenceHandle {
    tx: Mutex<Sender<Command>>,
    state: Arc<Mutex<PresenceDto>>,
}

impl PresenceHandle {
    pub fn set(&self, status: Option<PresenceStatus>, reconnect: bool) -> Result<(), String> {
        self.tx
            .lock()
            .map_err(|_| "presence channel poisoned".to_string())?
            .send(Command { status, reconnect })
            .map_err(|error| error.to_string())
    }

    pub fn state(&self) -> PresenceDto {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or(PresenceDto {
                connected: false,
                error: None,
                effective: None,
            })
    }
}

pub fn spawn(emit: impl Fn(PresenceDto) + Send + 'static) -> PresenceHandle {
    let (tx, rx) = channel();
    let state = Arc::new(Mutex::new(PresenceDto {
        connected: false,
        error: None,
        effective: None,
    }));
    let thread_state = state.clone();
    std::thread::spawn(move || run(rx, thread_state, emit));
    PresenceHandle {
        tx: Mutex::new(tx),
        state,
    }
}

fn run(rx: Receiver<Command>, state: Arc<Mutex<PresenceDto>>, emit: impl Fn(PresenceDto)) {
    // rustls 0.23 has no default crypto backend once built without one.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut desired: Option<PresenceStatus> = None;
    let mut backoff = 1u64;

    loop {
        loop {
            match rx.try_recv() {
                Ok(command) => desired = command.status,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        if desired.is_none() {
            match rx.recv() {
                Ok(command) => {
                    desired = command.status;
                    backoff = 1;
                }
                Err(_) => return,
            }
            continue;
        }
        let Some(token) = crate::account::load_token() else {
            set_state(
                &state,
                false,
                Some("Connect a warframe.market account in Settings to set your status.".into()),
                &emit,
            );
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(command) => desired = command.status,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            continue;
        };

        let started = Instant::now();
        let result = serve(&token, &mut desired, &rx, &state, &emit);
        if started.elapsed() > Duration::from_secs(30) {
            backoff = 1;
        }
        match result {
            // The desired status became None or a reconnect was asked for.
            Ok(()) => {
                if desired.is_none() {
                    set_state(&state, false, None, &emit);
                }
                backoff = 1;
            }
            Err(error) => {
                eprintln!("presence: {error}");
                set_state(&state, false, Some(error), &emit);
                std::thread::sleep(Duration::from_secs(backoff));
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

fn serve(
    token: &str,
    desired: &mut Option<PresenceStatus>,
    rx: &Receiver<Command>,
    state: &Arc<Mutex<PresenceDto>>,
    emit: &impl Fn(PresenceDto),
) -> Result<(), String> {
    let mut request = SOCKET_URL
        .into_client_request()
        .map_err(|error| format!("presence request: {error}"))?;
    request.headers_mut().insert(
        "Sec-WebSocket-Protocol",
        "wfm"
            .parse()
            .map_err(|error| format!("presence header: {error}"))?,
    );
    request.headers_mut().insert(
        "User-Agent",
        "coruframe/0.1"
            .parse()
            .map_err(|error| format!("presence header: {error}"))?,
    );
    let (mut socket, _) = connect(request).map_err(|error| format!("presence connect: {error}"))?;
    set_read_timeout(&mut socket);

    socket
        .send(Message::Text(
            json!({
                "route": "@wfm|cmd/auth/signIn",
                "payload": { "token": token },
                "id": "auth",
            })
            .to_string()
            .into(),
        ))
        .map_err(|error| format!("presence auth: {error}"))?;

    let mut authed = false;
    let mut sent: Option<&'static str> = None;
    let mut game_is_running = false;
    let mut last_scan: Option<Instant> = None;
    loop {
        loop {
            match rx.try_recv() {
                Ok(command) => {
                    *desired = command.status;
                    last_scan = None;
                    if command.reconnect {
                        let _ = socket.close(None);
                        return Ok(());
                    }
                    sent = None;
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = socket.close(None);
                    return Ok(());
                }
            }
        }
        let Some(status) = *desired else {
            let _ = socket.close(None);
            return Ok(());
        };
        if status == PresenceStatus::Auto
            && last_scan.is_none_or(|at| at.elapsed() >= Duration::from_secs(10))
        {
            game_is_running = game_running();
            last_scan = Some(Instant::now());
        }
        let wire = status.wire(game_is_running);
        if authed && sent != Some(wire) {
            socket
                .send(Message::Text(
                    json!({
                        "route": "@wfm|cmd/status/set",
                        "payload": { "status": wire },
                        "id": "status",
                    })
                    .to_string()
                    .into(),
                ))
                .map_err(|error| format!("presence status: {error}"))?;
            sent = Some(wire);
            eprintln!("presence: {wire}");
            set_effective(state, wire, emit);
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    match value.get("route").and_then(Value::as_str) {
                        Some("@wfm|cmd/auth/signIn:ok") => {
                            authed = true;
                            sent = None;
                            eprintln!("presence: connected");
                            set_state(state, true, None, emit);
                        }
                        Some("@wfm|cmd/auth/signIn:error") => {
                            *desired = None;
                            return Err(
                                "warframe.market rejected the session; reconnect in Settings."
                                    .into(),
                            );
                        }
                        Some("@wfm|event/auth/revoke") => {
                            *desired = None;
                            return Err("the warframe.market session ended.".into());
                        }
                        _ => {}
                    }
                }
            }
            Ok(Message::Ping(payload)) => {
                socket
                    .send(Message::Pong(payload))
                    .map_err(|error| format!("presence ping: {error}"))?;
            }
            Ok(Message::Close(_)) => return Err("warframe.market closed the socket.".into()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Err("warframe.market closed the socket.".into())
            }
            Err(error) => return Err(format!("presence: {error}")),
        }
    }
}

fn set_read_timeout(socket: &mut WebSocket<MaybeTlsStream<TcpStream>>) {
    let timeout = Some(POLL);
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(timeout);
        }
        MaybeTlsStream::Rustls(stream) => {
            let _ = stream.get_ref().set_read_timeout(timeout);
        }
        _ => {}
    }
}

// The game runs as a Windows binary under Proton, so its process name is the
// reliable signal; /proc/comm is cheap and catches it before the cmdline.
fn game_running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str() else { continue };
        if !pid.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let comm = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
        if comm.to_ascii_lowercase().contains("warframe") {
            return true;
        }
        let cmdline = std::fs::read(entry.path().join("cmdline")).unwrap_or_default();
        if cmdline.is_empty() {
            continue;
        }
        let cmdline = String::from_utf8_lossy(&cmdline).to_ascii_lowercase();
        if cmdline.contains("warframe") {
            return true;
        }
    }
    false
}

fn set_state(
    state: &Arc<Mutex<PresenceDto>>,
    connected: bool,
    error: Option<String>,
    emit: &impl Fn(PresenceDto),
) {
    let next = PresenceDto {
        connected,
        error,
        effective: None,
    };
    let Ok(mut guard) = state.lock() else { return };
    if *guard == next {
        return;
    }
    *guard = next.clone();
    drop(guard);
    emit(next);
}

fn set_effective(state: &Arc<Mutex<PresenceDto>>, effective: &str, emit: &impl Fn(PresenceDto)) {
    let Ok(mut guard) = state.lock() else { return };
    if guard.effective.as_deref() == Some(effective) {
        return;
    }
    guard.effective = Some(effective.to_string());
    let next = guard.clone();
    drop(guard);
    emit(next);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_follows_the_game() {
        assert_eq!(PresenceStatus::Auto.wire(false), "online");
        assert_eq!(PresenceStatus::Auto.wire(true), "ingame");
        assert_eq!(PresenceStatus::Invisible.wire(true), "invisible");
        assert_eq!(PresenceStatus::parse("auto"), Some(PresenceStatus::Auto));
    }
}
