use std::path::Path;

use rusqlite::{params, Connection};

use crate::parse::Event;

const SCHEMA_VERSION: i64 = 1;

pub struct Ledger {
    conn: Connection,
}

impl Ledger {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        // Create the file 0600 first: rusqlite would otherwise use the umask and
        // the journal would inherit the directory's mode.
        if let Err(error) = crate::private_fs::create_private(path) {
            return Err(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CANTOPEN),
                Some(format!("{}: {error}", path.display())),
            ));
        }
        let conn = Connection::open(path)?;
        // shortcut: pre-release schema reset, replace with migrations once a release ships
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version < SCHEMA_VERSION {
            conn.execute_batch("DROP TABLE IF EXISTS ledger;")?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS ledger (
                id INTEGER PRIMARY KEY,
                session TEXT NOT NULL,
                seq INTEGER NOT NULL,
                ts_ms INTEGER NOT NULL,
                game_ref TEXT NOT NULL,
                player_id TEXT NOT NULL,
                delta INTEGER NOT NULL,
                source TEXT NOT NULL,
                confidence TEXT NOT NULL,
                note TEXT NOT NULL
            );
            -- Uptime is stable within a session, so this dedupes a replayed log
            -- whether it was imported by `scan` or already tailed by `watch`.
            CREATE UNIQUE INDEX IF NOT EXISTS ledger_event
                ON ledger(session, ts_ms, game_ref, player_id);",
        )?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(Self { conn })
    }

    // The log names your own relic roll but never the reward you actually picked,
    // so a delta from the log alone is a lower bound until a snapshot reconciles it.
    pub fn record(&self, session: &str, event: &Event) -> rusqlite::Result<usize> {
        let Event::RelicReward {
            line,
            ts_ms,
            player_id,
            game_ref,
            ..
        } = event
        else {
            return Ok(0);
        };
        self.conn.execute(
            "INSERT OR IGNORE INTO ledger
                (session, seq, ts_ms, game_ref, player_id, delta, source, confidence, note)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, 'ee.log', 'unconfirmed',
                'relic roll; the reward actually picked is not logged')",
            params![session, *line as i64, *ts_ms as i64, game_ref, player_id],
        )
    }

    pub fn counts(&self) -> rusqlite::Result<Vec<(String, i64)>> {
        let mut stmt = self.conn.prepare(
            "SELECT game_ref, SUM(delta) FROM ledger GROUP BY game_ref ORDER BY game_ref",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reward(player_id: &str, ts_ms: u64, game_ref: &str) -> Event {
        Event::RelicReward {
            line: 7,
            ts_ms,
            player_id: player_id.into(),
            game_ref: game_ref.into(),
        }
    }

    fn temp_ledger(name: &str) -> (std::path::PathBuf, Ledger) {
        let dir =
            std::env::temp_dir().join(format!("coruframe-ledger-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let ledger = Ledger::open(&dir.join("ledger.db")).unwrap();
        (dir, ledger)
    }

    #[test]
    fn ledger_file_and_its_dir_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let root =
            std::env::temp_dir().join(format!("coruframe-ledger-modes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("nested");
        let _ledger = Ledger::open(&dir.join("ledger.db")).unwrap();
        let mode = std::fs::metadata(dir.join("ledger.db"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn counts_relic_rolls_once_per_replay() {
        let (dir, ledger) = temp_ledger("replay");
        let event = reward("3", 100_280, "/Lotus/StoreItems/Weapons/FulminPrimeBarrel");

        assert_eq!(ledger.record("session-a", &event).unwrap(), 1);
        assert_eq!(ledger.record("session-a", &event).unwrap(), 0);
        // A tailed session numbers lines from zero; a later scan has absolute numbers.
        assert_eq!(
            ledger
                .record(
                    "session-a",
                    &reward("3", 100_280, "/Lotus/StoreItems/Weapons/FulminPrimeBarrel")
                )
                .unwrap(),
            0
        );
        assert_eq!(
            ledger.counts().unwrap(),
            [("/Lotus/StoreItems/Weapons/FulminPrimeBarrel".to_string(), 1)]
        );

        assert_eq!(ledger.record("session-b", &event).unwrap(), 1);
        assert_eq!(
            ledger.counts().unwrap(),
            [("/Lotus/StoreItems/Weapons/FulminPrimeBarrel".to_string(), 2)]
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_timestamp_and_item_from_another_player_is_kept() {
        let (dir, ledger) = temp_ledger("player");
        let path = "/Lotus/StoreItems/Weapons/FulminPrimeBarrel";
        assert_eq!(ledger.record("s", &reward("3", 100_280, path)).unwrap(), 1);
        assert_eq!(ledger.record("s", &reward("1", 100_280, path)).unwrap(), 1);
        assert_eq!(ledger.counts().unwrap(), [(path.to_string(), 2)]);

        std::fs::remove_dir_all(&dir).ok();
    }
}
