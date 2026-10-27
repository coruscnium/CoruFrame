# Troubleshooting

Start with `coruframe doctor`. It prints the session, compositor hints, tools, capture backend, layer-shell support, Secret Service presence, OCR readiness and the cache/config/data modes. Include its output in a bug report.

## The HUD does not appear

Check, in order:

1. **Compositor.** The HUD needs `wlr-layer-shell`. KDE Plasma Wayland, sway and Hyprland have it. GNOME/Mutter does not; the HUD will not work there. `coruframe doctor` reports layer-shell support, and the app disables the HUD toggle when it is missing.
2. **Session.** The app needs a Wayland session. X11 and XWayland are not supported for the app; a Proton game may still run under XWayland.
3. **Game UI.** Only the English UI at 100% scale is calibrated. Another language or scale can produce `?` or no cards.
4. **Tesseract.** It must be installed with `eng`. `doctor` reports whether the binary and the model are both present.
5. **Portal grant.** The live probe uses the source you granted in the system portal. If the grant is stale or the wrong window was chosen, choose the Warframe window or the monitor it fills again.
6. **Frame shape.** A frame whose card row does not fit produces no HUD instead of a wrong crop. 16:9 is the live-validated family.
7. **The toggle.** Settings → Reward HUD must be on. It starts one `coruframe probe --hud` child and remembers the choice.

If the probe starts but exits, Settings shows the log tail and can open `~/.cache/coruframe/probe.log`. The log has the arm line, each capture, OCR text and matches, the HUD pid and the expiry.

## The HUD toggle fails to start

The app finds the CLI through `CORUFRAME_CLI`, the package's `usr/bin/coruframe`, a sibling binary, then `PATH`. A manual build that did not export `target/release` on `PATH` fails here. Set `CORUFRAME_CLI` to the absolute path of the built CLI, or add the directory to `PATH` before launching the app.

## A card shows `?`

`?` is deliberate. A card is priced only when the OCR consensus reaches the high confidence tier. A miss stays unknown rather than showing a plausible wrong price. Check the language and UI scale first, then the probe log for the OCR text and confidence.

## The window is white or blank

The app runs native Wayland with the WebKit sandbox on and the DMA-BUF renderer.

- If a driver white-screens with DMA-BUF, set `WEBKIT_DISABLE_DMABUF_RENDERER=1`. That keeps the sandbox and uses the software renderer.
- `CORUFRAME_UNSAFE_WEBKIT_SANDBOX=1` disables the sandbox and is only for a driver that cannot run sandboxed at all. The app logs a warning when it is set.
- A shell that replaces `XDG_DATA_DIRS` without the GSettings schemas can leave `gtk-xft-dpi` unset, after which WebKit computes a negative `devicePixelRatio` and lays the page out off-screen. The app prepends `GSETTINGS_SCHEMAS_PATH` to `XDG_DATA_DIRS` to avoid this.
- The app forces `GDK_BACKEND=wayland` when `WAYLAND_DISPLAY` is set, so a shell that exports `GDK_BACKEND=x11` does not start the webview under XWayland.

## Capture fails

- KWin: `kdotool` must be present. `coruframe install-desktop` adds the `ScreenShot2` allowlist entry for this binary.
- sway: install `swaymsg` and `grim`.
- Hyprland: install `hyprctl` and `grim`.
- X11: install `xdotool` plus `maim` or ImageMagick `import`.
- The portal path needs PipeWire. KDE rejects the `ActiveWindow` shortcut; pick the window or monitor explicitly.
- `CORUFRAME_CAPTURE_CMD` overrides both paths and runs through `sh -c`. A compositor call times out after 8 s, the override after 10 s and the portal after 15 s.

## The inventory is empty

The full inventory needs the opt-in snapshot and a game launched through the wrapper:

```sh
"/path/to/CoruFrame/target/debug/coruframe" launch -- %command%
```

Open the in-game inventory once after login so the synced inventory is populated. Between syncs the app shows the last capture and its capture and check times. If the wrapper is missing, the app stays on the log-ledger path and shows only what `EE.log` provides.

## The wallet is locked or will not open

A locked or dismissed wallet is reported in Settings with a Retry action. The app never falls back to a plaintext file while the wallet exists. Disconnect deletes both the wallet entry and the legacy file. See [Market-account](Market-account.md).

## Wrong prices or missing prices

Prices start from the local quote cache and fill asynchronously. Only high-confidence cards receive a price, so an unknown card has no price by design. If every price is missing, check that `coruframe catalog update` has run and that the market is reachable.

## Logs

| File | Contents |
|---|---|
| `~/.cache/coruframe/probe.log` | HUD probe stages, OCR text, matches, pids |
| `~/.cache/coruframe/ocr.log` | One JSON line per reward; no frames or tokens |
| `~/.cache/coruframe/inventory-scan.json` | Snapshot diagnostics |

All are 0600. The app's log view reads only `probe.log`; the webview cannot pass an arbitrary path to open.

## Questions

- **Does it run on Windows?** No. CoruFrame is Linux-only and targets Wayland.
- **Do I have to use Nix?** No. [Installation](Installation.md) covers a manual build for Debian, Ubuntu, Arch and Fedora.
- **Does it work on GNOME?** The app may run, but the HUD needs `wlr-layer-shell`, which GNOME/Mutter does not provide.
- **Can I be banned?** Digital Extremes' policy is that third-party software is used at your own risk. Read [Safety](Safety.md).
- **Where is the token stored?** In the Secret Service, with an owner-only file fallback. See [Market-account](Market-account.md).
- **What does the snapshot read?** Game memory, read-only and opt-in. See [Data-and-privacy](Data-and-privacy.md).
