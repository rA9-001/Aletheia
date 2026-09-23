# How Aletheia works

Browsers store cookies in a SQLite database and saved-login metadata alongside it. Aletheia
reads those for the current user's own profiles — copying locked files first, so the browser
can stay open — and summarises them.

A site is flagged **likely logged in** when it has a well-known login cookie (e.g.
`user_session` for GitHub) or several cookies whose names match session/auth patterns. That is
a heuristic, not a guarantee.

Deletions operate directly on the profile databases. Because a running browser can rewrite
cookies and passwords on exit, Aletheia detects when the browser is open and warns that changes
may not stick until you quit it.

## Privacy and scope

- Reads **only the profiles of the user running it** — no other users, machines, or remote data.
- **Never** reads, decrypts, logs, or transmits cookie values or passwords.
- No network activity of any kind.
- Writes only on explicit request; cache clearing is restricted to directories named `Cache` /
  `cache2` under your home directory.

Aletheia is a self-audit tool. Pointing it at someone else's data is out of scope and not
supported.

## Limitations

- "Has session cookies" is a heuristic, not proof of an active login.
- Per-site cache size isn't available — browsers store the cache as one opaque blob, so only
  the per-profile total is reported.
- macOS: profile paths are included, but cache size and the running-browser check are not
  implemented, and untested.

## The Python companion

`aletheia.py` is dependency-free and needs no build:

```bash
python3 aletheia.py            # full report
python3 aletheia.py --json     # machine-readable output
python3 aletheia.py --list     # list discovered profiles
```

## Build notes

If the AppImage step fails with a `strip: … .relr.dyn` error on a bleeding-edge distro, build
with `NO_STRIP=true cargo tauri build`. The included `./build.sh` wrapper does this for you;
`./install.sh` installs the binary with a desktop entry for the current user.

On Windows, WebView2 is already present on Windows 11; `cargo tauri build` produces an `.msi`
and an NSIS `.exe`.
