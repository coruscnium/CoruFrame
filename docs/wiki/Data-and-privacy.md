# Data and privacy

CoruFrame has no telemetry and no server of its own. Everything it reads stays on the machine.

## What it reads

- Warframe's `EE.log`, a plain local file, for login, loadout, mission-end and extraction-start events.
- Public APIs: warframe.market v2 (items, orders), warframestat drop tables and Digital Extremes' worldstate CDN.
- Screen capture for the reward OCR. The live HUD probe always uses the system portal's granted source (monitor or window), because that is the one source that works on every supported compositor; you grant it once in the portal picker.
- The `capture` command and `probe --now` use a PID-verified Warframe window through KWin's `ScreenShot2`, sway/Hyprland geometry plus `grim`, or `xdotool` plus `maim` on X11. `CORUFRAME_CAPTURE_CMD` replaces both for debugging.

Without the opt-in snapshot it reads no game memory at all. It never writes game memory, never injects into the game, never synthesizes input and never modifies game files.

## Where it is stored

All directories are 0700 and files 0600.

| Path | Contents |
|---|---|
| `~/.cache/coruframe/itemdb.json` | Compact item database, about 3.3 MB |
| `~/.cache/coruframe/quotes-v2.json` | Cached market quotes |
| `~/.cache/coruframe/inventory-raw.json` | Inventory snapshot, when enabled |
| `~/.cache/coruframe/inventory-status.json` | Content hash, capture times and last scan state |
| `~/.cache/coruframe/inventory-scan.json` | Trimmed scan diagnostics |
| `~/.cache/coruframe/probe.log` | HUD probe stages; no frame bytes or tokens |
| `~/.cache/coruframe/ocr.log` | One JSON line per reward; rotates at 2 MB |
| `~/.cache/coruframe/ocr-frames/`, `ocr-corpus/`, `ocr-crops/` | Opt-in debug and evaluation data |
| `~/.config/coruframe/` | Token fallback, device id, account marker, HUD settings |
| `~/.local/share/coruframe/ledger.db` | SQLite ledger of log events |

The market token normally lives in the freedesktop Secret Service, not in `~/.config`. See [Market-account](Market-account.md).

## Retention

- Debug OCR frames and crops are deleted after seven days; the normal session sweep also removes a kept live session on a later probe start.
- `ocr.log` rotates at 2 MB. It holds sizes, card rectangles, OCR text and confidences, never frame bytes, player names or tokens.
- The corpus is local-only; its frames are ignored by git and only the manifest is committed.
- The ledger, snapshots and cache stay until you delete them.
- Removing everything means deleting `~/.cache/coruframe`, `~/.config/coruframe` and `~/.local/share/coruframe`, and disconnecting the market account first so the wallet entry is removed.

## Debug switches

| Variable | Effect |
|---|---|
| `CORUFRAME_SNAPSHOT_DEBUG=1` | Restores memory snippets in the snapshot diagnostics |
| `CORUFRAME_KEEP_OCR_SESSION=1` | Keeps the HUD session crops after the HUD exits |
| `CORUFRAME_KEEP_OCR_FRAMES=all` or `miss` | Keeps full debug frames |

Debug data is opt-in, 0600 and never uploaded.

## Item database

`coruframe itemdb update` fetches the per-category JSON from [WFCD/warframe-items](https://github.com/WFCD/warframe-items) (MIT) and builds the compact `~/.cache/coruframe/itemdb.json` that the app reads. The raw categories total about 15 MB; the compact file is about 3.3 MB and is parsed once, off the UI thread.

The data is Digital Extremes' game data. WFCD's library is MIT-licensed; Warframe's data and images remain Digital Extremes' property.

The market catalog stays the pricing and slug source. It is joined by the game's `uniqueName` (`/Lotus/...`), never filtered by the WFCD `tradable` flag: built prime items are marked non-tradable while their market set is tradable, and WFCD also marks Necramech parts non-tradable even though the market lists them. Unresolved paths are shown as Unknown instead of being dropped.

## Inventory snapshot (opt-in)

The snapshot is off for fresh installs. It reads the game process memory `O_RDONLY`; it never attaches a tracer, writes to the game, or touches game files. An install that already has `inventory-raw.json` and no stored preference keeps working and shows a one-time notice.

Requirements:

- CoruFrame must be an ancestor of `Warframe.x64.exe`, so launch the game through the wrapper.
- The host needs `kernel.yama.ptrace_scope` at 1 (the parent traces its child) or
  0. At 2 the read is disabled unless the process has `CAP_SYS_PTRACE`.

With no existing launch prefix:

```sh
# Steam launch option; quote the path because it contains a space
"/path/to/CoruFrame/target/debug/coruframe" launch -- %command%
```

If your launch options already have a gamescope prefix, keep it and put the wrapper immediately before `%command%`:

```sh
gamescope -f -W 3440 -H 1440 -- "/path/to/CoruFrame/target/debug/coruframe" launch -- %command%
```

What happens next:

- The wrapper writes `inventory-raw.json` only when the inventory content changes, and `inventory-status.json` on every scan.
- The first scan waits 25 s. After that, login or relay-load lines in `EE.log` start a 30 s burst that scans every 2 s; a complete capture ends the burst.
- The app reloads the 800 KB raw file only when the content hash changes.
- The game holds the profile JSON only while it syncs, so between syncs the app shows the last full capture and its capture and check times. Opening the in-game inventory or changing relay triggers the next sync.
- Extraction re-reads are capped and the report records offsets and counts, never the whole memory. Snippets are empty unless `CORUFRAME_SNAPSHOT_DEBUG=1`.
- Token, secret, session and id keys are replaced with `[redacted]` before a report is written.

A snapshot written by a newer schema is marked as newer and its unknown sections are ignored.

The snapshot is EULA §2.h grey area and host-observable. See [Safety](Safety.md).
