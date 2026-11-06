# Contributing

## Build and test

Build the macOS client with:

```bash
cargo build -p nook
```

Run the workspace tests with:

```bash
cargo test --workspace --no-fail-fast
```

Before opening a PR, run `cargo fmt` and `cargo clippy --workspace --all-targets`.

Permission-gated features need a real app bundle because macOS TCC attaches grants to a bundle identifier. Build one with `./scripts/bundle.sh` (`./scripts/installer.sh` wraps it in a DMG).

## Layout and design

`crates/nook-core` is the platform/back-end layer. `crates/nook-mods` is the mod runtime (QuickJS). `crates/nook` is the Rune island, the services mods read, and the shipped mods.

Keep the dot-matrix look for coding-agent faces and maps; everything else uses a clean macOS style. Test with Liquid Glass on and off.

Shipped mods live in `crates/nook/mods` and are embedded at build time; `cargo run -p nook -- check <mod folder>` and `cargo run -p nook -- shot --mod <name>` show what a change does without running the island. `./scripts/screens.sh` re-renders the README screenshots.

## Pull requests and issues

Open an issue for a focused bug or proposal. For changes, explain the user-visible behavior, keep the diff focused, and include the checks you ran. Open a PR against `main` and respond to review feedback before merging.
