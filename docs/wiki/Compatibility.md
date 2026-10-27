# Compatibility

Run `coruframe doctor` first: it prints the session, compositor hints, tools, the chosen capture backend, layer-shell support, Secret Service presence and the cache/config/data modes.

## Matrix

| Session | Capture command | HUD | Status |
|---|---|---|---|
| KWin / Plasma (Wayland) | `kdotool` window UUID + KWin `ScreenShot2.CaptureWindow` | `wlr-layer-shell` | Supported and verified live (3440x1440 capture, OCR, HUD spawn) |
| KWin / Plasma (X11) | Same D-Bus capture | Unsupported (X11 gate) | Capture best-effort, HUD off |
| sway | `swaymsg` tree/outputs for geometry, `grim -o` full output, crop in Rust | `wlr-layer-shell` | Best-effort; geometry parser unit-tested, no live session yet |
| Hyprland | `hyprctl clients -j` / `monitors -j`, `grim -o`, crop in Rust | `wlr-layer-shell` | Best-effort; geometry parser unit-tested, no live session yet |
| GNOME (Wayland) | Portal only, explicit | No `wlr-layer-shell` | **The HUD does not work and is not supported.** Capture is explicit-only |
| Generic X11 | `xdotool` geometry, `maim` or ImageMagick `import` | No `wlr-layer-shell` | Capture best-effort; the HUD does not work and is not supported |
| Any | `CORUFRAME_CAPTURE_CMD` via `sh -c` | As above | Trusted override for debugging |

The HUD probe always uses the portal source; the Capture command column describes the `capture` command's window backend. A wrong-window capture is never produced there: every window is verified through `/proc/<pid>/cmdline` or `environ` (Warframe or Steam AppID 230410), and a frame without a verified window is an error. `probe --now` uses the same PID-verified auto backend.

## Tools

| Tool | Used for | Required |
|---|---|---|
| `kdotool` | KWin window search, PID and geometry | KWin |
| `tesseract` | Reward-card OCR | HUD |
| `grim` | Full-output capture + window crop | sway / Hyprland |
| `swaymsg` | sway geometry | sway |
| `hyprctl` | Hyprland geometry | Hyprland |
| `xdotool` | X11 window search, PID and geometry | X11 |
| `maim` or `import` | X11 window capture | X11 |
| `gtk4`, `gtk4-layer-shell` | Reward HUD | HUD |
| `curl` | All HTTP | Market, catalog, worldstate |

The devshell provides `kdotool`, `tesseract`, `grim`, `spectacle`, GTK4 and layer-shell. `nix build` produces the app and the CLI with Tesseract on their `PATH`; the `nix run .#probe` wrapper puts the runtime binaries and libraries on the path for an already-built `target/` binary. The Nix package wraps the CLI with Tesseract and `eng`, and links GTK4, layer-shell and PipeWire.

## Reward HUD and OCR

The **English** game UI at **100% UI scale** is the only supported setup. Other scales and languages will be added as compatibility work when they are reported; they are not supported yet.

The HUD requires **`wlr-layer-shell`**. Compositors without it, notably GNOME (Mutter), are **not supported**: the HUD does not work there. X11/XWayland is also unsupported. `coruframe doctor` and the app's capabilities panel report whether layer-shell and Tesseract are present.

Every visible card is drawn. A card is only priced when the OCR consensus reaches the high confidence tier; otherwise the HUD shows `?`. Tesseract and its `eng` data must be installed.

## Wallet

The token uses freedesktop Secret Service through `keyring` 4.2 (`zbus-secret-service-keyring-store`, `crypto-rust`, no OpenSSL). On KDE this resolves to the `kdewallet` collection; it is verified live with a throwaway entry (write, read, delete) and with the real token migration.

If the service is absent, the token is stored in `~/.config/coruframe/market-token` (0600) and Settings reports `storage = "file"`. A locked or cancelled wallet never falls back to the file.

## Portal caveats

- KDE rejects `ActiveWindow`; choose the Warframe window (or the monitor it fills) in the portal picker.
- The portal response does not name the output, so the HUD cannot remap it to another monitor.
- `--backend` only affects the `capture` command; the HUD probe always uses the granted portal source.

## Environment switches

| Variable | Effect |
|---|---|
| `CORUFRAME_CAPTURE_CMD` | Overrides capture; receives the output path in `CORUFRAME_OUT`; runs via `sh -c` |
| `CORUFRAME_UNSAFE_WEBKIT_SANDBOX=1` | Disables the WebKit sandbox; logged, for an unfixable driver only |
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | Keeps the sandbox, uses the software renderer |
| `CORUFRAME_TOKEN_STORE=file` | Pins the plaintext file store for one release |
| `CORUFRAME_SNAPSHOT_DEBUG=1` | Restores memory snippets in the wrapper's diagnostic report |

## Requirements summary

See [Installation](Installation.md) for the dependency blocks per distribution and for the manual build.
