# OCR corpus

`manifest.json` names the frames and the exact item each reward card shows. The frames themselves are **not** committed: they are game screenshots and can contain squad player names.

## Collection

1. Play a Void Fissure to the reward screen in the English UI, 100% UI scale.
2. Set `CORUFRAME_KEEP_OCR_FRAMES=all` (or `miss`) before the probe.
3. The probe writes a 0600 frame and a `<stem>.json` metadata file under `~/.cache/coruframe/ocr-frames/`. It never uploads them and deletes both after seven days.
4. Copy the frames into `~/.cache/coruframe/ocr-corpus/` (0700), or the directory you pass to `--corpus`, and add one manifest entry per frame.
5. Write down the expected names in left-to-right card order. Use the catalog's canonical name (`Braton Prime Barrel`, not `Braton Prime barrel`).

## Manifest

```json
{
  "version": 1,
  "frames": [
    {
      "file": "2026-10-26-kompressa.png",
      "width": 3440,
      "height": 1440,
      "ui_scale": 100,
      "language": "en",
      "game_version": "2026.10.22",
      "source": "local",
      "expected": [
        "Forma Blueprint",
        "Paris Prime Upper Limb",
        "Lex Prime Receiver",
        "Kompressa Prime Blueprint"
      ],
      "notes": "Kompressa card is two-line; gold-ish art behind the name"
    }
  ]
}
```

`language` must be `en` for the 1.0 gate. `width`/`height` are checked against the frame and reported when they differ.

## Evaluation

```sh
direnv exec . cargo run -- ocr-eval \
  --corpus ~/.cache/coruframe/ocr-corpus \
  --manifest ~/.cache/coruframe/ocr-corpus/manifest.json \
  --report /tmp/ocr-report.md
```

The command exits nonzero when a gate fails. The gates are card count, exact name (global and per resolution/UI-scale setup), zero wrong prices, unknown rate, and p95 latency. The local reference set is third-party from `github.com/soranew/wfinfo-linux`; use it locally until its provenance is settled.

## Privacy

- Frames stay local, 0600, and are excluded by `.gitignore`.
- The debug capture is opt-in; production writes no frame at all.
- The committed manifest contains item names only, never player names.
- Logs carry card rectangles, OCR text and confidences, not frame bytes.
