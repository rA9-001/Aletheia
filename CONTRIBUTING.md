# Contributing

Thanks for your interest in improving Aletheia!

## Ground rules

This is a **privacy** tool. Contributions must preserve its core guarantees:

1. **Local & own-profile only** — never read another user's data, another
   machine's data, or anything remote.
2. **No secrets** — never read, decrypt, log, or transmit cookie values or
   passwords. Metadata (names, flags, expiry, domains) only.
3. **No network** — the app must not make outbound network calls or add
   telemetry.
4. **Destructive actions are explicit** — any delete/clear must stay behind a
   confirmation and must validate its target path.

PRs that weaken these will not be merged.

## Project layout

- `src-tauri/src/scan.rs` — all scanning + deletion logic (cross-platform
  `cfg` branches for Linux/Windows/macOS).
- `src-tauri/src/lib.rs` — Tauri commands exposed to the frontend.
- `ui/` — vanilla HTML/CSS/JS frontend (no framework, no build step).
- `aletheia.py` — standalone Python CLI that mirrors the core logic.

## Dev setup

See the README for prerequisites. Then:

```bash
cargo tauri dev            # hot-reload window
cd src-tauri && cargo test # run unit tests (delete logic uses throwaway DBs)
```

Before opening a PR:

- `cargo build` and `cargo test` pass.
- `cargo clippy` is clean (or explain remaining warnings).
- `cargo fmt` applied.
- New platform paths are guarded with `#[cfg(target_os = "...")]`.

## Commit style

Small, focused commits with a clear message. Describe *why*, not just *what*.
