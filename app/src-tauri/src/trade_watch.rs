use std::time::Duration;

use tauri::{AppHandle, Emitter};

use coruframe_core::logpath;
use coruframe_core::tail::Follower;
use coruframe_core::trade::Parser;

// EE.log is the only place the game writes what both sides offered. A trade
// dialog is multi-line, so the parser buffers it and only emits after the
// success marker.
pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || loop {
        let Some(path) = logpath::find() else {
            std::thread::sleep(Duration::from_secs(2));
            continue;
        };
        let mut follower = Follower::new(path, false);
        let mut parser = Parser::default();
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let Ok(lines) = follower.poll() else {
                break;
            };
            if follower.last_poll_truncated() {
                parser = Parser::default();
            }
            for line in lines {
                if let Some(event) = parser.feed(&line) {
                    let _ = app.emit("trade-completed", &event);
                }
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    });
}
