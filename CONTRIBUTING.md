# Contributing

Bug reports and patches are welcome. CoruFrame is a local Linux desktop app, so the most useful report names the session, the compositor, the version and the `coruframe doctor` output.

## Before you start

Read the [Compatibility page](https://github.com/coruscnium/CoruFrame/wiki/Compatibility). The supported setup is narrow: KDE Plasma Wayland (or another `wlr-layer-shell` compositor), the English game UI at 100% scale. A bug on GNOME, X11 or another UI scale is still worth reporting, but it is out of scope for a fix until compatibility work is scheduled.

Security issues go to [SECURITY.md](SECURITY.md), not a public issue. Never paste a real market token, a wallet entry, or a screenshot containing another player's name.

## Toolchain

Nix with flakes, plus direnv:

```sh
direnv allow
cargo test
```

The devshell provides Rust, Node, pnpm, Tesseract, GTK4 and `gtk4-layer-shell`. `app/src-tauri` is its own Cargo workspace; run its commands from that directory.

## Gates

A change must pass both workspaces and the frontend:

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

The UI audit rejects raw `px` font sizes, cryptic order notation and dead settings labels. Use the design tokens in `app/src/style.css`. The docs check verifies that every relative link and image target in the repository Markdown exists.

Frontend coverage is `vue-tsc --noEmit` (part of `pnpm build`) plus the UI audit; there is no component or store unit-test suite yet. A change to a store or an API wrapper is verified by running the app, and the pull-request template has a slot for naming what was exercised.

The OCR corpus is local-only. Do not add frames to the repository or to build artifacts; `ocr-eval` skips the corpus gate when it is absent. If a change touches OCR, run it against your local corpus and describe the result in the PR.

## AI assistance

AI slop and vibe-coding are not accepted. Low-effort generated output, or iterating prompts until something passes without reading the result, is not a contribution. This applies to code, tests, documentation, issues, pull requests and review comments, though this list isn't exhaustive and generally applies to the entire repo.

AI-generated assets are not accepted. Generated images, icons, art, audio, fonts, models, datasets or other non-code media are not accepted. Do not submit generated material that is infringing, deceptive or otherwise unethical or irresponsible.

AI-assisted code, tests and documentation are permitted only when you understand them. You remain the author and are accountable for the whole change, including the generated parts. You must be able to explain what the change does, why it is there and how you verified it, and answer review questions in your own words.

Any violation of this rule regarding AI assistance will result in a ban, without exceptions. Do not act as a pass-through for a model.

## Commits and pull requests

- One logical change per commit. Imperative, sentence-case subject; no `feat:` or `fix:` prefix; no attribution trailers.
- Keep the diff scoped. A refactor that is not needed for the fix belongs in its own PR.
- New logic with a branch, loop, parser, money or security gets a small test.
- When dependencies change, run `cargo audit` (install it with `cargo install cargo-audit`) against both lockfiles and `pnpm audit` locally. New dependencies need a reason; the project prefers the standard library or an existing crate.
- Update the affected docs in the same change. The user guide is the GitHub wiki, updated separately from the code and not reviewed in a pull request; contributor and legal records stay in the repository.
- Verify pull requests on a Nix system. There is deliberately no per-commit CI; local gates and the tag-only release workflow are the checks.
- Do not commit a token, a wallet entry, a captured frame or a local path.

## Commit authorship

Commit as yourself. Do not add a `Co-Authored-By` trailer for an assistant and do not fake an email address; the repository owner reviews the author field before merging.

## Release

Releases are cut from a `vX.Y.Z` tag whose version matches all four manifests. The release workflow checks the tag, runs the tests, builds the Nix package and creates a GitHub release from the source tag.

Before tagging:

- Add the release date to [CHANGELOG.md](CHANGELOG.md).
- Re-check the published wiki Installation and Usage pages against the tag.
- Refresh any screenshots in the README and the wiki when the UI changed.
- Push the tag and confirm the release notes match the curated changelog section.
