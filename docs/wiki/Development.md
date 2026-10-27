# Development

CoruFrame has two Cargo workspaces: the root workspace with `crates/core` and `crates/cli`, and `app/src-tauri` with the Vue frontend in `app/src`.

## Start here

[CONTRIBUTING.md](https://github.com/coruscnium/CoruFrame/blob/main/CONTRIBUTING.md) covers the toolchain, gates, commit conventions and the release checklist.

## Gates

```sh
direnv exec . cargo fmt --all -- --check
direnv exec . cargo clippy --workspace --all-targets -- -D warnings
direnv exec . cargo test --workspace

cd app/src-tauri
direnv exec ../.. cargo fmt --all -- --check
direnv exec ../.. cargo clippy --all-targets -- -D warnings
direnv exec ../.. cargo test

cd ..
pnpm build
cd ..
node scripts/ui-audit.mjs
node scripts/check-docs.mjs
```

The OCR corpus is local-only. A change that touches OCR runs `ocr-eval` against the local corpus and describes the result in the pull request.

## Documentation

The user guide is `docs/wiki/` and is updated in the same change as the code. Contributor-facing records stay in the repository: `CONTRIBUTING.md`, `SECURITY.md` and `docs/legal/`.
