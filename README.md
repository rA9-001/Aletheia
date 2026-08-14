<p align="center">
  <img src="docs/aletheia-icon.png" alt="Aletheia" width="128" />
</p>

<h1 align="center">Aletheia</h1>

<p align="center">
  A local desktop app that shows which sites your browsers keep you logged into,
  which hold saved passwords, and how much each profile caches — and lets you
  clean any of it up.<br/>
  Runs entirely on your machine. No network. No telemetry.
</p>

<p align="center">
  <a href="https://github.com/rA9-001/Aletheia/actions/workflows/ci.yml"><img src="https://github.com/rA9-001/Aletheia/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <img src="https://img.shields.io/badge/platform-Linux%20%7C%20Windows-lightgrey" alt="Platform" />
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License" />
</p>

---

Aletheia **never reads, decrypts, or transmits** your passwords or cookie
*values* — it works only from cookie names, flags, and metadata, and it makes no
network calls of any kind.

<!-- Add a screenshot from a throwaway/example profile:
![Aletheia](docs/screenshot.png) -->

## Features

- **Audit** — per profile: likely logged-in sites, saved-password sites, and
  total cache size.
- **Cookie details** — expand any site to see its cookies with expiry and
  `Secure` / `HttpOnly` flags.
- **Flags** — highlights **stale sessions** (unused for 60+ days) and known
  **third-party trackers**.
- **Cleanup** — remove a site's cookies, delete a saved password, or clear a
  profile's cache. Bulk-remove trackers or everything at once, with a
  **keep-list** to protect sites you choose. Every deletion is confirmed.
- **Browsers** — Firefox, LibreWolf, and Chromium-family (Brave, Chrome,
  Chromium, Vivaldi, Edge).
- **Platforms** — Linux and Windows.

## Install

Download an installer from the [Releases](../../releases) page:

- **Linux** — `.AppImage` (portable), `.deb`, or `.rpm`
- **Windows** — `.msi` or `.exe`

Or build from source below.

## Build from source

Requires the [Rust toolchain](https://rustup.rs) and the Tauri CLI
(`cargo install tauri-cli --version "^2"`).

**Linux** — also install `webkit2gtk-4.1`, `gtk3`, `librsvg2`, and
`libsoup-3.0` development packages, then:

```bash
cargo tauri dev      # run in a dev window
cargo tauri build    # build installers (.deb / .rpm / .AppImage)
```

> If the AppImage step fails with a `strip: … .relr.dyn` error (some
> bleeding-edge distros), build with `NO_STRIP=true cargo tauri build`. The
> included `./build.sh` wrapper does this for you; `./install.sh` installs the
> binary with a desktop entry for the current user.

**Windows** — install the *Microsoft C++ Build Tools* (Desktop development with
C++); WebView2 is already present on Windows 11. Then run `cargo tauri build`
(produces an `.msi` and an NSIS `.exe`). Builds must be produced on their target
OS; the release workflow does this automatically for all platforms.

## How it works

Browsers store cookies in a SQLite database and saved-login metadata alongside
it. Aletheia reads those for the current user's own profiles (copying locked
files first, so the browser can stay open) and summarizes them. A site is
flagged **likely logged in** when it has a well-known login cookie (e.g.
`user_session` for GitHub) or several cookies whose names match session/auth
patterns — a heuristic, not a guarantee.

Deletions operate directly on the profile databases. Because a running browser
can rewrite cookies and passwords on exit, Aletheia detects when the browser is
open and warns that changes may not stick until you quit it.

## Privacy & scope

- Reads **only the profiles of the user running it** — no other users, machines,
  or remote data.
- **Never** reads, decrypts, logs, or transmits cookie values or passwords.
- No network activity of any kind.
- Writes only on explicit request; cache clearing is restricted to directories
  named `Cache` / `cache2` under your home directory.

Aletheia is a self-audit tool. Pointing it at someone else's data is out of
scope and not supported.

## Limitations

- "Has session cookies" is a heuristic, not proof of an active login.
- Per-site cache size isn't available — browsers store the cache as one opaque
  blob, so only the per-profile total is reported.
- macOS: profile paths are included but cache size and the running-browser
  check are not implemented (untested).

## CLI

`aletheia.py` is a dependency-free Python companion for scripting and JSON
output:

```bash
python3 aletheia.py            # full report
python3 aletheia.py --json     # machine-readable output
python3 aletheia.py --list     # list discovered profiles
```

## Feedback

This project doesn't accept external contributions. If you've found a bug or
want a feature, please [open an issue](https://github.com/rA9-001/Aletheia/issues).

## Security

Report vulnerabilities privately as described in [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE).
