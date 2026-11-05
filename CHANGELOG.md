# Changelog

Notable changes per release. Versions follow the git tag (`vX.Y.Z`); the tag and all four manifests must agree, which `scripts/version-check.mjs` enforces.

## 1.0.3 (2026-11-05)

### Added

- The reward overlay caches the prices it fetches in `hud-quotes.json` and reuses them, so only cards still missing a price cost a request. It also reads the app's fetched prices for its first paint.
- While the overlay is on screen it holds the app's background market queue, so its card prices are never queued behind inventory or the recent-orders feed.

### Changed

- Inventory rows are priced only from their own rank's listings (or the item's headline when it has no ranks). A rank with no orders now shows no price instead of borrowing another rank's.
- A reward card the matcher identified is drawn with its icon, set and parts at every confidence tier. It shows a price when the tier is high or when the OCR text is an exact, unique match for the item's name.

### Fixed

- The recent-orders feed no longer prices anything; it only feeds the Market tab's trending list. Guessed variants are gone and the quote cache moved to `quotes-v4.json`, so the inventory total no longer moves with cached estimates.

## 1.0.2 (2026-11-05)

### Added

- `ocr-eval` corpus frames accept an optional `squad` hint and score the hinted layout path alongside the unhinted one.

### Changed

- Reward-card geometry is derived from the frame: the row band is a fraction of the frame height, the card width scales with it, and the row is centered on the width, so 16:9 is not assumed.

### Fixed

- The reward row is resolved from the frame, not the squad size. Squad size is only an upper bound now, so a player whose relic failed to crack no longer leaves a phantom card; the HUD scores the four-, three-, two- and one-card layouts and keeps the hinted count when a smaller layout would drop a visible card.
- The Worldstate panel no longer sits at Offline. warframestat sends plain alert reward items as bare strings, and a single one failed the whole document parse even though the fetch succeeded. Only `countedItems` are objects.
- Untracking every market item keeps the list empty after a reload instead of restoring the default seed items, which treated a saved empty array as a fresh install.

## 1.0.1 (2026-10-27)

### Added

- CI runs the gates and the Nix package build on every push to `main` and every pull request.

### Changed

- The user guide lives on the GitHub wiki now; the in-repo guide pages were removed and links and issue templates point there.
- The README has a hero image and app screenshots, and credits p33p4w for the icon art.
- The sidebar version no longer carries the stale "local only" marker.
- Nix install examples pin the flake input to a release tag, so users move on releases instead of every commit.
- Dependency refresh: PipeWire 0.10, Vite 8, vue-router 5, vue-tsc 3 and current GitHub Actions.

### Fixed

- Riven cards show the weapon thumbnail instead of the generic placeholder, and fall back to the Riven symbol for veiled or unknown weapons; the Rivens counter uses the Riven symbol too.

## 1.0.0 (2026-10-27)

First public release.

### Added

- Relic-reward HUD on `wlr-layer-shell`: capture, OCR, item matching, prices and a corner panel that closes after the 20 s reward window.
- Reward OCR pipeline built on Tesseract with per-card confidence. A card is priced only at the high tier; everything else is drawn as `?`. Confidence, variant consensus, geometry detection and the three capture backends are part of the OCR pipeline.
- `ocr-eval` corpus scoring, `ocr-crops` evidence export and opt-in debug capture.
- Desktop app: Collection browser with mastery from `XPInfo`, inventory ledger, relic planner, Rivens, Trading orders, Settings and the reward-HUD toggle.
- Warframe account integration: Secret Service token storage, presence, order creation and updates, with an owner-only file fallback.
- Opt-in read-only inventory snapshot with hash-based change detection and redacted diagnostics.
- `coruframe doctor`, capabilities JSON, portal source selection and `install-desktop`.
- A Nix package (`nix build`) for the full app: GUI, CLI, desktop entry, icons, WebKit, GTK4/layer-shell/PipeWire and Tesseract with `eng`.
- Warframe's trade dialog is parsed from EE.log. A completed item-for-platinum trade marks the matching WFM sell listing sold, and an item received for platinum marks the matching buy listing. Auto-mark can be turned off in Settings.
- The Trading page has a Mark sold action that closes one trade stack.

### Changed

- The live HUD probe uses the system portal source; `capture` and `probe --now` keep the PID-verified window backends.
- Reward names come from per-card OCR with variants and consensus instead of a whole-row pass.
- HUD prices are asynchronous: cards draw first and quotes fill in.

### Fixed

- The raw inventory snapshot keeps item paths instead of redacting long names, so the relic planner sees every owned relic.
- Only one HUD probe can run. A duplicate exits, the probe dies with the app, and old probes no longer stack screen requests.
- The relic planner ignores Requiem relics.
- WFM orders keep `perTrade`. Prices are shown per unit and per trade, and the seller dialog computes the trade total instead of treating a per-trade price as a unit price.

### Known limitations

- English game UI at 100% scale only. Other languages and scales are not calibrated.
- HUD requires `wlr-layer-shell`; GNOME/Mutter and X11 do not work.
- The live matrix has been verified for a four-card fissure at 3440×1440 and 1080p. Solo and 2–3 card squads, the Kompressa two-line card and other resolutions are untested; the gate status is tracked with the release.
- Official support is the Nix flake. A manual source build is documented but not officially supported; report issues, but try Nix first.

### Notes

- Source is offered at the tag; the GPL-3.0 text is in [LICENSE](LICENSE).
- Third-party licenses are in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) and [LICENSES/](LICENSES/).
