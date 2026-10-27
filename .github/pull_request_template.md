## What changed

<!-- One or two sentences. Link the issue if there is one. -->

## Verification

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` at the root
- [ ] The same three commands in `app/src-tauri`
- [ ] `pnpm build` in `app`; `node scripts/ui-audit.mjs` and `node scripts/check-docs.mjs` from the repository root
- [ ] OCR change: `ocr-eval` against a local corpus, result described below
- [ ] HUD or capture change: tested in a live session, session and compositor named below
- [ ] User-facing change: Wiki updated, or this change is not user-facing

## Notes for the reviewer

<!-- Risks, untested paths, or a decision the reader should check. Do not paste tokens, wallet entries or frames containing other players' names. -->
