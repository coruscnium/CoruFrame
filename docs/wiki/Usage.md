# Usage

## First run

```sh
coruframe doctor                 # what this system can do
coruframe itemdb update          # build the item database cache
coruframe catalog update         # build the market catalog
coruframe-app                    # open the app, then enable the HUD in Settings
```

`doctor` prints the detected session, compositor hints, tools, capture backend, layer-shell support, Secret Service presence, OCR readiness and the cache/config/data modes. It is the first thing to include in a bug report.

## The app

The desktop app has Collection, Inventory, Relic Planner, Rivens, Trading and Settings. Run the production build rather than the Vite dev runtime; the dev build is much slower on large inventories.

Settings → Reward HUD has the enable toggle. Turning it on starts one `coruframe probe --hud` child through the CLI resolver: `CORUFRAME_CLI`, the package's `usr/bin/coruframe`, a sibling binary, then `PATH`. The app keeps the child running while it is open and remembers the choice. Turning the toggle off kills the probe's process group, which also ends an HUD that is already on screen. A start failure or a probe that exits shows the log tail and can open `~/.cache/coruframe/probe.log`. The toggle is disabled when the capabilities panel reports no layer-shell or no Tesseract.

## The portal grant

The live HUD probe captures the source you grant in the system portal, once. Choose the Warframe window (or the monitor it fills) in the picker. That is the one capture source that works on every supported compositor.

KDE rejects the `ActiveWindow` shortcut; pick the window or monitor explicitly. The portal response does not name the output, so the HUD cannot remap it to another monitor.

The `capture` command and `probe --now` use a different, PID-verified window backend: KWin `ScreenShot2`, sway/Hyprland geometry plus `grim`, or `xdotool`/`maim` on X11. A frame is only produced for a verified Warframe window; a browser tab with the same title is never captured.

## Reward HUD

The probe tails `EE.log` every 250 ms. A `RelicExtraction` line arms it, and `AddSquadMember squadCount=N` fixes the expected card count. It captures the portal source, crops the reward row, OCRs each card and draws a corner panel for 20 seconds.

- Each visible card is drawn. A card is priced only when the OCR consensus reaches the high confidence tier; everything else shows `?`.
- The first capture is at 4 s and only opens the HUD when every expected card is high confidence. A 5 s retry accepts unknown cards, and the probe keeps trying every second for up to 20 s if the frame has no cards.
- Prices fill asynchronously; the cards are already on screen.
- Mini part counts read `owned/required`, so a barrel shows `x0/2` rather than a bare `x0`.

The probe appends a timestamped stage log (start line, arm, each capture, OCR text and matches, HUD pid, expiry) to `~/.cache/coruframe/probe.log`. Pass `--log-file PATH` to move it. The app's HUD toggle uses the same log and can show its tail or open the file when the probe exits with an error.

Supported: KDE Plasma Wayland and other compositors with `wlr-layer-shell` (sway, Hyprland). GNOME/Mutter, X11/XWayland and anything without layer-shell do not work and are not supported. The English UI at 100% scale is the only calibrated setup.

A HUD that does not appear is covered in [Troubleshooting](Troubleshooting.md).

## Commands

```sh
coruframe logpath
coruframe scan  --log PATH/TO/EE.log --db /tmp/cf.db
coruframe watch --db /tmp/cf.db --json
coruframe probe --hud                     # capture + OCR, draw the corner reward HUD
coruframe probe --now                     # capture immediately
coruframe probe --image FRAME             # OCR a saved frame, print each card and confidence
coruframe capture --out DIR               # capture one verified frame and print its path
coruframe capture --backend wlr           # force kwin|wlr|x11|portal
coruframe doctor                          # session, tools, backend, wallet, OCR, modes
coruframe install-desktop                 # KWin ScreenShot2 allowlist for this binary
coruframe ocr-eval --corpus DIR --manifest FILE  # score OCR against a local corpus
coruframe ocr-crops --frame FRAME         # export card/name-strip PNGs and payload
coruframe catalog update
coruframe itemdb update                    # full WFCD item database cache
coruframe itemdb audit                     # resolve the snapshot against the item DB
coruframe price "Fulmin Prime Barrel"
coruframe relic "Axi A5" --price
```

`watch` tails `EE.log` every 250 ms, survives the truncation on every game launch, and dedupes against a later `scan` of the same log using the launch timestamp in the log header.

## Signing in

The warframe.market account is optional and only needed for orders and presence. See [Market-account](Market-account.md).

## Inventory and mastery

The full inventory and mastery need the opt-in snapshot, which is off for fresh installs. See [Data-and-privacy](Data-and-privacy.md) for the setup and what it reads.
