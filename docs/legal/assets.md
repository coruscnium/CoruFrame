# Third-party assets

Everything in this file is excluded from the GPL-3.0 grant that covers the CoruFrame source. The licence texts are in [LICENSES/](../../LICENSES/); the Rust crate attributions are in [dependencies.md](dependencies.md).

## CoruFrame icon

`app/src-tauri/icons/icon.png` and [coruframe-icon.png](../images/coruframe-icon.png) are original CoruFrame artwork by p33p4w, licensed under GPL-3.0-only like the source. They are not third-party assets.

## Warframe art and data

The PNG images under `app/src/assets/wf/` and `crates/cli/assets/` are Warframe user-interface artwork:

- `app/src/assets/wf/` — category glyphs, currency symbols, planet and star-chart art (including the `Icon*_256.png` family), sourced from WFCD's image set at `cdn.warframestat.us` and the Warframe Wiki, downscaled for display.
- `crates/cli/assets/Platinum.png` — the platinum symbol the HUD draws.
- `crates/cli/assets/PartPlaceholder.png` — the HUD's missing-art placeholder, drawn as a weapon-part silhouette.

Warframe and its assets are © Digital Extremes. They are used here for non-commercial fan content, in line with Digital Extremes' Content Policy and Fan Merchandise Policy:

- https://www.warframe.com/en/contentpolicy
- https://www.warframe.com/en/fan-merchandise-policy

CoruFrame is free, non-commercial and not affiliated with Digital Extremes. These files remain Digital Extremes' property and may be removed on request. The CoruFrame brand mark is original to this project.

`app/src-tauri/data/riven-base.json` is a small derived table of riven stat coefficients (base values, prefixes and suffixes per stat). The table was extracted with WFCD's MIT-licensed tooling; the numeric values are Warframe game data, not WFCD's original work. It is redistributed under the same non-commercial fan-content position as the art above, not under the GPL.

These assets are used under Digital Extremes' fan-content policy for a free, non-commercial, unofficial companion. Digital Extremes does not endorse third-party software and is not affiliated with CoruFrame. The files remain Digital Extremes' property and will be removed on request.

The screenshots in [../images/](../images/) are captures of CoruFrame. The Warframe art and game data they show remain Digital Extremes' property under the same notice.

Item names, relic names, drop tables and the market catalog are fetched from public APIs at runtime and are not redistributed. The item database cache (`~/.cache/coruframe/itemdb.json`, built from [WFCD/warframe-items](https://github.com/WFCD/warframe-items)) is generated on the user's machine and is not committed.

## Lucide

The Help/About line glyph in `app/src/lib/icons.ts` is Lucide's `circle-question-mark` geometry (https://lucide.dev), Copyright (c) 2026 Lucide Icons and Contributors, licensed under the ISC license. The other line icons are original to this project. Text: [LICENSES/ISC.txt](../../LICENSES/ISC.txt).

## Tesseract

The reward OCR shells out to Tesseract, licensed under the Apache License 2.0 (https://github.com/tesseract-ocr/tesseract). Tesseract is a runtime dependency, not vendored into the source tree: Nix provides the binary and the `eng` model, and the Nix package wraps the CLI with Tesseract plus `eng.traineddata`, its `configs/` and `tessconfigs/`. A manual build uses a system Tesseract. The `eng` model is trained by the Tesseract project; see its `tessdata` repository for the model license and provenance. Text: [LICENSES/Apache-2.0.txt](../../LICENSES/Apache-2.0.txt).

The optional second engine (RapidOCR/PaddleOCR ONNX) is not shipped. If it is adopted, record its detector/recognizer model hashes, sizes and licenses here before adding it to the package.

## WFCD

The item database, relic data and image URLs come from [WFCD/warframe-items](https://github.com/WFCD/warframe-items), Copyright (c) 2017 Kaptard, MIT licensed. Text: [LICENSES/MIT-WFCD.txt](../../LICENSES/MIT-WFCD.txt).

CoruFrame's generated `eng.user-words` list is built at runtime from the public catalog and WFCD item data; it is not committed.

## OCR reference frames

Game screenshots used for OCR regression stay local under `~/.cache/coruframe/ocr-corpus/` and are never part of a release or this repository. The five local reference frames come from `github.com/soranew/wfinfo-linux`; their provenance is unresolved, so they are used locally only and are not redistributed.
