# openNook GPUI security audit

**Target:** `pre-release` @ `8b2a6a5d0f38d753d511c11483cbcd524c14c28d` (`https://github.com/prodBirdy/openNook`)
**Scope:** GPUI/Rust desktop overlay (`crates/nook`, `crates/nook-core`). Tauri is treated as retired leftover except where it can still ship secrets, IPC, or update artifacts.
**Date:** 2026-10-01
**Auditor:** Firstmate fleet kickoff (static review + `cargo audit`)

## Grade: B

openNook’s GPUI path is a local, highly privileged overlay (Accessibility, Apple Events, optional Full Disk Access, Messages `chat.db`, a real PTY) with a small, mostly well-defended remote/local-IPC surface. There is no plugin/code loader, no standing daemon socket, and the `opennook://` scheme is explicitly designed so browsers cannot reach the shell. Secrets on macOS go to Keychain; the SQLite store is chmod `0600`. The one High finding is a still-live Tauri `Release` workflow with `contents: write` on every `main` push and `v*` tag. Combined with a Medium rustls advisory, LocalSend TLS that disables certificate checks without using the fingerprint pin it already implements, and plaintext Linux token files, this is a solid B — not an A. Nothing in this pass is Critical.

## Findings

| ID | Severity | Area | Evidence | Impact | Recommended fix |
| --- | --- | --- | --- | --- | --- |
| OA-01 | **High** | Update channel | `.github/workflows/release.yml:1-64` — workflow name `Release`, triggers `workflow_dispatch` + tags `v*` + push to `main`, job `publish-tauri`, `permissions.contents: write`, `tauri-apps/tauri-action@v0`, `bun-version: latest` | Official “Release” job still tries to publish unsigned Windows/Linux Tauri installers under `openNook v__VERSION__`. Current tree has no `package.json` / `src-tauri`, so `bun install` fails today — but any reintroduced frontend, or a confused operator using this workflow, can ship unsigned binaries next to real GPUI tags. Every `main` push and `v*` tag starts this job. | Delete or disable this workflow. Keep `linux-release.yml` + local `scripts/bundle.sh` as the only publish paths. Draft PR: retire Tauri `Release` workflow. |
| OA-02 | Medium | Dependencies | `Cargo.lock` `rustls 0.23.43`; `cargo audit` **RUSTSEC-2026-0285** (GHSA-2mjx-qc3c-rqvc), CVSS 5.3, patched `>=0.23.45` | TLS 1.3 handshake messages accepted across encryption-level boundaries. Transcript is still authenticated, so a network attacker cannot complete a forged handshake; a peer can send should-be-encrypted handshake bytes in plaintext. rustls is in the lockfile (GPUI / hyper-rustls / quinn); app HTTP also uses reqwest default `native-tls` (`openssl 0.10.81`). | `cargo update -p rustls --precise 0.23.45` (or newer) and commit the lockfile. |
| OA-03 | Medium | Local IPC / TLS | `crates/nook-core/src/share/localsend.rs:438-445`, `:784-788` (`danger_accept_invalid_certs` + `danger_accept_invalid_hostnames`); `:462-475` `verify_tls_fingerprint` (tests only) | Send path talks HTTPS to LAN peers with CA verification off and never pins the announced SHA-256 fingerprint. A LAN MITM can see files the user chose to send. Official LocalSend’s trust model is self-signed + fingerprint; this copies the “accept invalid certs” half only. Discovery also binds TCP/UDP on `0.0.0.0` for ~3s (`:318-354`). | Call fingerprint verification on the TLS peer cert before upload, or drop HTTPS and use HTTP-only on the LAN with a clear UI warning. Do not leave the helper unused. |
| OA-04 | Medium | Secrets | `crates/nook-core/src/spotify.rs:732-757` — non-macOS writes refresh token to `app_data_dir()/spotify-refresh` via `std::fs::write` with no `0o600` | Default umask `022` makes the file world-readable. Any local user can steal the Spotify refresh token and control playback. macOS correctly uses Keychain (`:723-729`, `:741-750`). | Mirror `database::restrict_db_mode`: create with `0o600`, `chmod` after write. Prefer secret-service on Linux. |
| OA-05 | Medium | Update channel | `.github/workflows/linux-release.yml:54-75` — `softprops/action-gh-release@v2` uploads a tarball, no checksum artifact, no sigstore/minisign; `scripts/bundle.sh:71-78` ad-hoc signs when Developer ID is missing | Linux GPUI channel is unsigned `cargo build --release`. There is no in-app updater, so MITM requires tricking the user at download time (GitHub HTTPS helps). macOS production depends on whoever runs `bundle.sh` having a Developer ID + optional `NOTARY_PROFILE`. Draft `v1.0.0` dmg exists; old Tauri tag `0.0.2a` is still a public release. | Publish SHA-256 sums (and a signature) next to the tarball. Keep Tauri `0.0.2a` clearly marked retired on the Releases page. Never ad-hoc-sign bits you intend people to download. |
| OA-06 | Low | Secrets | `.env:1` (`NOOK_DEBUG_HITBOX=1`); `.gitignore` does not ignore `.env`; git history only ever contained this debug flag (`3970418` → rename in `7f1a1ca`) | Not a credential. Prior “tracked `.env` leftovers” hypothesis is a debug switch, not an API key. Still a footgun if someone later puts a token in `.env`. | Add `.env` to `.gitignore` and stop tracking the file. Document `NOOK_DEBUG_HITBOX` in CONTRIBUTING. |
| OA-07 | Low | Local IPC | `crates/nook-core/src/spotify.rs:528-562` — first TCP accept on `127.0.0.1:43821` wins; state is checked after the HTML response | Classic loopback OAuth race: another local process can connect first, steal the code (PKCE verifier still required, so they also need this process’s verifier — they cannot finish the exchange without it). Residual: they can DoS the flow. PKCE + `state` (`:559-561`, `:586-608`) are otherwise sound. | Prefer `SO_REUSEADDR` off (already unique port), accept only until a matching `state`, do not write success HTML before validation. |
| OA-08 | Low | Secrets / crypto | `crates/nook-core/src/spotify.rs:196-211` — `/dev/urandom` failure falls back to time-based LCG | PKCE verifier/`state` become predictable if urandom is unavailable. Unlikely on macOS/Linux. | Fail closed (`Err`) if `read_exact` fails; do not invent entropy. |
| OA-09 | Low | File / network | `crates/nook-core/src/observe.rs:388-399` — any `http://` or `https://` URL; default `https://api.warmup-gamelauncher.com` (`:36-38`); bearer from Settings / `WARMUP_METRICS_TOKEN` / `ADMIN_METRICS_TOKEN` (`:433-443`) | User-configured Prometheus URL is a first-party SSRF (link-local, intranet). Default warmup host is third-party and only contacted when the Observe widget is on *and* a bearer is present. Linux persists a non-empty metrics token in SQLite (`:208-211` skip_serializing is macOS-only). | Scheme + host allowlist; refuse link-local/metadata IPs. Store Linux tokens with `0600` (or secret-service). Keep warmup default opt-in. |
| OA-10 | Low | File / network | `crates/nook-core/src/utils.rs:36-63` `fetch_artwork_from_url` — any URL, 5 MiB cap, no host allowlist; Linux MPRIS `mpris:artUrl` at `audio.rs:917-928`; queue art at `queue.rs:66` | A player can point artwork at an intranet URL. Safari path *is* allowlisted (`browser_media.rs:458+`); Spotify AppleScript path requires `https://` (`audio.rs:289-291`). | Reuse the Safari host allowlist (or https-only + public suffix) for all artwork fetches. Reject `file:` / `http://127.0.0.1`. |
| OA-11 | Low | Local IPC | `crates/nook-core/src/automation.rs:52-157`, `Info.plist:57-66` — `opennook://` registered; any webpage can `tray/add`, `tray/clear`, `timer/start`, `expand`, `settings` | No auth (Launch Services). Paths must exist and are canonicalized (`:160-173`). Shell verbs are rejected (`:54-56`, `:129-130`). Impact is UI abuse / pre-staging a sensitive existing path in the tray for the user to send. | Optional confirmation for tray-add from a URL. Leave the forbid-list in place. |
| OA-12 | Low | File access | `docs/examples/external-counter-plugin/README.md` — instructs copy to `~/.opennook/plugins/` and `window.__openNookPluginAPI__` | Dead Tauri/React plugin story. Grep of `*.rs` finds **no** plugin loader, no `~/.opennook/plugins`, no JS eval. Misleading docs only. | Delete or mark the example “Tauri-era, not loaded by GPUI”. |
| OA-13 | Info | Dependencies | `cargo audit`: unmaintained `async-std`, `instant`, `paste`, `proc-macro-error2`, `rustls-pemfile`, `rustybuzz`, `serial`, `ttf-parser` (transitive, mostly GPUI/fonts) | No known exploit path from unmaintained status alone. | Track via `cargo audit` in CI; bump GPUI when it moves off these. |
| OA-14 | Info | Dependencies | `Cargo.toml:45-46` path-patch `third_party/vt100`; `scripts/build-mediaremote-adapter.sh:9` pins `ungive/mediaremote-adapter` @ `3ac3d4b`; **no** `source = "git"` in `Cargo.lock`; **no** `build.rs` in workspace | Supply-chain surface is crates.io + one documented path patch + one pinned git clone at bundle time (not linked; Perl loads the framework). | Keep the pin. Do not switch the adapter to a floating branch. |
| OA-15 | Info | Secrets | macOS Keychain: Spotify (`spotify.rs:23-25`), warmup bearer (`settings.rs:1167-1218`); metrics token omitted from JSON on macOS (`observe.rs:207-208`); DB `0600` (`database.rs:45-55`); no hardcoded API keys found; logs mention token *errors* only | Good. Linux warmup token can land in `opennook.db` (OA-09). Fallback DB path `tmp/opennook-gpui-fallback.db` is also chmod `0600`. | None on macOS. See OA-04/OA-09 for Linux. |
| OA-16 | Info | File access / privilege | Opt-in Messages (`messages.rs` reads `~/Library/Messages/chat.db`), Notifications AX + optional usernoted FDA (`notifications.rs:1-8`), Agents reads Cursor `state.vscdb` / Grok session files (`agents.rs:1160+`), Obsidian `vault_join` rejects `..` (`obsidian.rs:679-688`), Terminal PTY unreachable from URLs (`shell.rs:3-4`) | By-design local privilege. Bodies are not written to `opennook.db` (watermarks only). World-writable dirs were not created by the app. | Keep widgets off by default (already true for Messages / Notifications / Agents / Observe). |
| OA-17 | Info | Local IPC | No Unix socket, no D-Bus service exported by openNook, no localhost HTTP server at idle. Spotify loopback is bind-on-connect (`127.0.0.1:43821`). Linux media uses zbus as an **MPRIS client**. LocalSend listeners are send-only and dropped after ~3s. Accessibility is used for HUD/meeting controls and notification scrape, not as an IPC server. CLI is `open -g opennook://` (`crates/nook/src/bin/nook.rs`) | Privilege boundary is OS user + TCC, not an extra auth token. | Keep it that way. Do not add a daemon socket. |
| OA-18 | Info | Telemetry | No Sentry/analytics/crash-reporter crate. Outbound HTTPS: Open-Meteo, LRCLIB, iTunes/AMP (scraped anonymous JWT in `motion_artwork.rs:241-250`), Cloudflare/OVH speed test, optional warmup/Prometheus, Spotify API | AMP JWT scrape is a ToS/stability issue, not a user-secret leak. Speed test downloads ~25–100 MB when the user runs it. | Disclose AMP scrape in Settings copy if not already. |

## Prioritized fix list

### P0

1. **OA-01** — Retire `.github/workflows/release.yml` so `v*` / `main` cannot invoke `tauri-action`. (Draft PR opened from this audit.)

### P1

2. **OA-02** — Bump `rustls` to `>=0.23.45`.
3. **OA-03** — Enforce LocalSend fingerprint pin on the upload/scan TLS client, or stop advertising HTTPS.
4. **OA-04** — `0600` (or secret-service) for Linux `spotify-refresh`.
5. **OA-05** — Checksums (and a signature) on the Linux tarball; label leftover Tauri `0.0.2a` as retired on the Releases page.

### P2

6. **OA-06** — Untrack `.env`, gitignore it.
7. **OA-07 / OA-08** — Tighten Spotify loopback + fail closed on RNG failure.
8. **OA-09 / OA-10** — Allowlist observe + artwork URLs.
9. **OA-12** — Remove or retitle the Tauri plugin example.

## What was checked

- Workspace crates (`crates/nook`, `crates/nook-core`), `Cargo.toml` / `Cargo.lock`, `third_party/`, `scripts/`, `Info.plist`, `resources/openNook.entitlements`, `.github/workflows/*`, `.env` history, `docs/examples/`.
- Pattern search for sockets/listeners, osascript, Keychain, tokens, plugins, `build.rs`, git crate sources, Tauri leftovers, updater/Sparkle.
- `cargo audit` against RustSec advisory-db (1278 advisories, fetched 2026-10-01). Full log: `/opt/cursor/artifacts/cargo-audit.txt`.
- `gh release list` / `gh release view v1.0.0` (draft dmg only; public history still includes Tauri `0.0.2a` and GPUI `v0.3.0-linux`).
- Line-level read of Spotify OAuth, LocalSend, `opennook://` parser, settings persist, observe URL normalization, files tray, Obsidian `vault_join`, database mode, shell PTY comments, notifications/messages/meetings modules.

## What was not checked

- No runtime fuzz of the OAuth loopback server, LocalSend discovery, or `opennook://` ingestion.
- No live MITM of LocalSend or rustls handshakes.
- No execution of the macOS bundle (this environment is Linux); TCC, Keychain ACLs, codesign, and notarization were read from scripts/plist only.
- No binary review of published `openNook-1.0.0.dmg` or `0.0.2a` Tauri installers (metadata only).
- No attempt to run the retired Tauri workflow (would need `package.json` / `src-tauri`, which are absent).
- No review of GitHub Actions secret store beyond what workflows reference (`GITHUB_TOKEN` only).
- GPUI/`blade`/Metal shader code was not treated as an untrusted-input parser beyond noting rust-embed of local assets.

## Hypothesis check (prior notes)

| Prior note | Current state |
| --- | --- |
| Tracked `.env` leftovers | **Confirmed file, not a secret.** Only `NOOK_DEBUG_HITBOX=1`, same since first add. |
| Dirty Tauri `release.yml` vs main | **Confirmed still live on `pre-release`.** Same workflow is on `pre-release` (and listed on the repo). It is the official `Release` name, still points at Tauri, still has `contents: write`. It cannot succeed on this tree today (`bun install` has no frontend). |

## High / Critical fix PRs

| Finding | PR |
| --- | --- |
| OA-01 Tauri `Release` workflow | See companion draft PR on `cursor/retire-tauri-release-workflow-ff67` (opened with this audit). |

No Critical findings. Medium and below are documented here only — no fix PRs.
