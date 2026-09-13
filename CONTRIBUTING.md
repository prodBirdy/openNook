# Contributing

## Build and test

Build the macOS client with:

```bash
./scripts/with-metal.sh cargo build
```

Run the workspace tests with:

```bash
./scripts/with-metal.sh cargo test --workspace --no-fail-fast
```

Before opening a PR, run `cargo fmt` and `cargo clippy --workspace --all-targets`.

Permission-gated features need a real app bundle because macOS TCC attaches grants to a bundle identifier. Build one with `./scripts/with-metal.sh ./scripts/bundle.sh`.

## Layout and design

`crates/nook-core` is the platform/back-end layer. `crates/nook` is the GPUI UI and island client.

Keep the dot-matrix look only for coding-agent faces; everything else uses a clean macOS style. Test painted surfaces and Liquid Glass on supported systems.

Experimental widgets live behind the Settings toggle.

## Pull requests and issues

Open an issue for a focused bug or proposal. For changes, explain the user-visible behavior, keep the diff focused, and include the checks you ran. Open a PR against `main` and respond to review feedback before merging.
