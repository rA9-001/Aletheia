# Aletheia

> *alétheia* (ἀλήθεια) — truth as *un-concealment*: not a claim that is correct, but a thing
> brought out of hiding.

A desktop app that shows which sites your browsers keep you logged into, which hold saved
passwords, and how much each profile caches — and lets you clean any of it up. Runs entirely
on your machine: no network calls of any kind, and it never reads, decrypts or transmits your
passwords or cookie *values*, only names, flags and metadata.

[![build](https://github.com/rA9-001/Aletheia/actions/workflows/ci.yml/badge.svg)](https://github.com/rA9-001/Aletheia/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/rA9-001/Aletheia?sort=semver)](https://github.com/rA9-001/Aletheia/releases/latest)
[![licence](https://img.shields.io/github/license/rA9-001/Aletheia)](LICENSE)

## Install

Download from [Releases](https://github.com/rA9-001/Aletheia/releases/latest) — `.AppImage`,
`.deb` or `.rpm` on Linux, `.msi` or `.exe` on Windows. Or build from source below.

## What it does

| | |
| --- | --- |
| **Audit** | Per profile: likely logged-in sites, saved-password sites, total cache size |
| **Cookie details** | Expand any site for its cookies, with expiry and `Secure` / `HttpOnly` |
| **Flags** | Stale sessions (unused 60+ days) and known third-party trackers |
| **Cleanup** | Remove a site's cookies, a saved password, or a profile's cache — bulk or one at a time, with a keep-list. Every deletion is confirmed |
| **Browsers** | Firefox, LibreWolf, and Chromium-family (Brave, Chrome, Chromium, Vivaldi, Edge) |

`aletheia.py` is a dependency-free Python companion for scripting: `--json` for
machine-readable output, `--list` to enumerate profiles.

## Building

Requires the [Rust toolchain](https://rustup.rs) and the Tauri CLI
(`cargo install tauri-cli --version "^2"`).

```bash
cargo tauri dev      # run in a dev window
cargo tauri build    # build installers
```

On Linux also install `webkit2gtk-4.1`, `gtk3`, `librsvg2` and `libsoup-3.0` development
packages; `./build.sh` wraps the AppImage quirks. On Windows install the *Microsoft C++ Build
Tools*. Builds must be produced on their target OS; the release workflow does all platforms.

## Docs

[How it works](docs/internals.md) — detection heuristics, privacy scope, limitations, platform notes.

Vulnerabilities: see [SECURITY.md](SECURITY.md). This project doesn't accept external
contributions — please [open an issue](https://github.com/rA9-001/Aletheia/issues) instead.

## License

MIT. See [LICENSE](LICENSE).
