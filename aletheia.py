#!/usr/bin/env python3
"""
Aletheia — a local browser privacy-audit tool.

Scans the browser profiles belonging to the user running it and reports:
  * Logged-in sites   — domains that hold session/login cookies
  * Saved credentials — sites you saved a password for (metadata only)
  * Cache usage       — how much disk each browser's HTTP cache uses

Design principles / ethics:
  * It only ever reads the LOCAL user's own profiles. It does not touch other
    users, other machines, or anything remote.
  * It never decrypts or displays password or cookie *values*. It reports which
    sites have credentials/sessions, not the secrets themselves.
  * It copies locked SQLite files to a temp dir before reading, so you don't
    have to close the browser.

Standard library only. Tested on Linux with Firefox and Chromium-family
browsers (Brave, Chrome, Chromium). macOS/Windows paths are included but less
exercised.
"""

from __future__ import annotations

import argparse
import configparser
import json
import os
import shutil
import sqlite3
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

# ----------------------------------------------------------------------------
# Browser / profile discovery
# ----------------------------------------------------------------------------

HOME = Path.home()


def _chromium_roots() -> dict[str, list[Path]]:
    """Config-dir roots for Chromium-family browsers, per platform."""
    roots: dict[str, list[Path]] = {}
    if sys.platform.startswith("linux"):
        base = HOME / ".config"
        candidates = {
            "Brave": [base / "BraveSoftware" / "Brave-Browser"],
            "Chrome": [base / "google-chrome"],
            "Chromium": [base / "chromium"],
            "Vivaldi": [base / "vivaldi"],
            "Edge": [base / "microsoft-edge"],
        }
    elif sys.platform == "darwin":
        base = HOME / "Library" / "Application Support"
        candidates = {
            "Brave": [base / "BraveSoftware" / "Brave-Browser"],
            "Chrome": [base / "Google" / "Chrome"],
            "Chromium": [base / "Chromium"],
            "Edge": [base / "Microsoft Edge"],
        }
    elif os.name == "nt":
        base = Path(os.environ.get("LOCALAPPDATA", HOME / "AppData" / "Local"))
        candidates = {
            "Brave": [base / "BraveSoftware" / "Brave-Browser" / "User Data"],
            "Chrome": [base / "Google" / "Chrome" / "User Data"],
            "Chromium": [base / "Chromium" / "User Data"],
            "Edge": [base / "Microsoft Edge" / "User Data"],
        }
    else:
        candidates = {}

    for name, paths in candidates.items():
        existing = [p for p in paths if p.is_dir()]
        if existing:
            roots[name] = existing
    return roots


def _firefox_roots() -> list[Path]:
    if sys.platform.startswith("linux"):
        return [p for p in (HOME / ".mozilla" / "firefox",
                            HOME / ".librewolf") if p.is_dir()]
    if sys.platform == "darwin":
        return [p for p in (HOME / "Library" / "Application Support" / "Firefox",
                            HOME / "Library" / "Application Support" / "LibreWolf")
                if p.is_dir()]
    if os.name == "nt":
        base = Path(os.environ.get("APPDATA", HOME / "AppData" / "Roaming"))
        return [p for p in (base / "Mozilla" / "Firefox",) if p.is_dir()]
    return []


@dataclass
class Profile:
    browser: str        # e.g. "Brave", "Firefox"
    family: str         # "chromium" or "firefox"
    name: str           # profile name / directory name
    path: Path          # profile directory
    cache_path: Path | None = None  # HTTP cache dir (may live elsewhere)


def discover_profiles() -> list[Profile]:
    profiles: list[Profile] = []

    # --- Chromium family ------------------------------------------------------
    for browser, roots in _chromium_roots().items():
        for root in roots:
            # Profiles are subdirs containing a "Cookies" or "Preferences" file:
            # "Default", "Profile 1", "Profile 2", ...
            for child in sorted(root.iterdir()):
                if not child.is_dir():
                    continue
                if not (child / "Preferences").exists() and not (child / "Cookies").exists():
                    continue
                cache = _chromium_cache_dir(root, child.name)
                profiles.append(Profile(browser, "chromium", child.name, child, cache))

    # --- Firefox family -------------------------------------------------------
    for root in _firefox_roots():
        browser = "LibreWolf" if "librewolf" in str(root).lower() else "Firefox"
        ini = root / "profiles.ini"
        prof_dirs: list[Path] = []
        if ini.exists():
            cp = configparser.ConfigParser()
            try:
                cp.read(ini)
                for section in cp.sections():
                    if not cp.has_option(section, "Path"):
                        continue
                    rel = cp.get(section, "Path")
                    is_rel = cp.getboolean(section, "IsRelative", fallback=True)
                    prof_dirs.append((root / rel) if is_rel else Path(rel))
            except (configparser.Error, ValueError):
                pass
        if not prof_dirs:  # fall back to globbing
            prof_dirs = [p for p in root.glob("*.*") if p.is_dir()]

        for p in prof_dirs:
            if not p.is_dir():
                continue
            cache = _firefox_cache_dir(root, p)
            profiles.append(Profile(browser, "firefox", p.name, p, cache))

    return profiles


def _firefox_cache_dir(root: Path, profile_dir: Path) -> Path | None:
    """Firefox/LibreWolf HTTP cache is in ~/.cache/<app>/<profile>/cache2 on
    Linux; on macOS/Windows it sits inside the profile as cache2."""
    if sys.platform.startswith("linux"):
        try:
            rel = root.relative_to(HOME)  # ".mozilla/firefox" or ".librewolf"
        except ValueError:
            rel = None
        bases = []
        if rel is not None and rel.parts:
            # ".mozilla/firefox" -> "mozilla/firefox"; ".librewolf" -> "librewolf"
            bases.append(HOME / ".cache" / Path(rel.parts[0].lstrip("."), *rel.parts[1:]))
            bases.append(HOME / ".cache" / rel.parts[-1].lstrip("."))
        for base in bases:
            cand = base / profile_dir.name / "cache2"
            if cand.exists():
                return cand
    cand = profile_dir / "cache2"
    return cand if cand.exists() else None


def _chromium_cache_dir(root: Path, profile_name: str) -> Path | None:
    """Chromium's HTTP cache lives in ~/.cache on Linux, not the config dir."""
    if sys.platform.startswith("linux"):
        # Map ~/.config/X -> ~/.cache/X
        try:
            rel = root.relative_to(HOME / ".config")
            cand = HOME / ".cache" / rel / profile_name / "Cache"
            if cand.exists():
                return cand
        except ValueError:
            pass
    # macOS / Windows keep it in-profile
    cand = root / profile_name / "Cache"
    return cand if cand.exists() else None


# ----------------------------------------------------------------------------
# Helpers
# ----------------------------------------------------------------------------

def _open_locked_sqlite(db_path: Path, tmpdir: Path) -> sqlite3.Connection | None:
    """Copy a possibly-locked SQLite DB to a temp file and open read-only."""
    if not db_path.exists():
        return None
    dst = tmpdir / f"{db_path.parent.name}_{db_path.name}"
    try:
        shutil.copy2(db_path, dst)
        # Copy the WAL too if present, so we see recent writes.
        for suffix in ("-wal", "-shm"):
            side = db_path.with_name(db_path.name + suffix)
            if side.exists():
                shutil.copy2(side, dst.with_name(dst.name + suffix))
        return sqlite3.connect(f"file:{dst}?mode=ro", uri=True)
    except (OSError, sqlite3.Error):
        try:  # last resort: open the original read-only
            return sqlite3.connect(f"file:{db_path}?mode=ro", uri=True)
        except sqlite3.Error:
            return None


def _dir_size(path: Path) -> int:
    total = 0
    try:
        for entry in path.rglob("*"):
            try:
                if entry.is_file():
                    total += entry.stat().st_size
            except OSError:
                continue
    except OSError:
        pass
    return total


def human_size(n: int) -> str:
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if n < 1024 or unit == "TB":
            return f"{n:.0f} {unit}" if unit == "B" else f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} TB"


def registrable_domain(host: str) -> str:
    """Cheap eTLD+1 approximation (no public-suffix list dependency)."""
    host = host.lstrip(".")
    parts = host.split(".")
    if len(parts) <= 2:
        return host
    # Handle common two-level TLDs (co.uk, com.au, ...).
    two_level = {"co", "com", "org", "net", "gov", "edu", "ac"}
    if len(parts) >= 3 and parts[-2] in two_level and len(parts[-1]) == 2:
        return ".".join(parts[-3:])
    return ".".join(parts[-2:])


# ----------------------------------------------------------------------------
# Cookie / login-session analysis
# ----------------------------------------------------------------------------

# Cookie-name substrings that strongly suggest an authenticated session.
SESSION_HINTS = (
    "session", "sess", "sid", "auth", "token", "login", "logged",
    "secure", "account", "identity", "remember", "__host-", "__secure-",
    "csrf",  # weak on its own, but presence of many auth-ish cookies matters
)
# Well-known per-site login cookies (name -> friendlier signal).
KNOWN_LOGIN_COOKIES = {
    "li_at",            # LinkedIn
    "sessionid",        # Instagram, Django sites
    "phpsessid",        # generic PHP
    "jsessionid",       # generic Java
    "__secure-1psid",   # Google
    "sapisid",          # Google
    "c_user",           # Facebook
    "xs",               # Facebook
    "ds_user_id",       # Instagram
    "reddit_session",   # Reddit
    "twid",             # Twitter/X
    "auth_token",       # Twitter/X
    "user_session",     # GitHub
    "_gh_sess",         # GitHub
}


@dataclass
class DomainCookieInfo:
    domain: str
    cookie_count: int = 0
    session_score: int = 0
    matched_names: set[str] = field(default_factory=set)
    has_session_cookie: bool = False  # cookie with no expiry (browser-session)

    @property
    def likely_logged_in(self) -> bool:
        return self.session_score >= 2 or bool(self.matched_names & KNOWN_LOGIN_COOKIES)


def analyze_cookies(profile: Profile, tmpdir: Path) -> list[DomainCookieInfo]:
    if profile.family == "chromium":
        db = _open_locked_sqlite(profile.path / "Cookies", tmpdir)
        name_col, host_col, expiry_expr = "name", "host_key", "expires_utc"
        table = "cookies"
    else:
        db = _open_locked_sqlite(profile.path / "cookies.sqlite", tmpdir)
        name_col, host_col, expiry_expr = "name", "host", "expiry"
        table = "moz_cookies"
    if db is None:
        return []

    result: dict[str, DomainCookieInfo] = {}
    try:
        rows = db.execute(
            f"SELECT {name_col}, {host_col}, {expiry_expr} FROM {table}"
        ).fetchall()
    except sqlite3.Error:
        db.close()
        return []
    db.close()

    for name, host, expiry in rows:
        if not host:
            continue
        dom = registrable_domain(host)
        info = result.setdefault(dom, DomainCookieInfo(dom))
        info.cookie_count += 1
        lname = (name or "").lower()
        if expiry in (0, None):
            info.has_session_cookie = True
        for hint in SESSION_HINTS:
            if hint in lname:
                info.session_score += 1
                info.matched_names.add(lname)
                break
        if lname in KNOWN_LOGIN_COOKIES:
            info.session_score += 2

    return sorted(result.values(), key=lambda i: (not i.likely_logged_in, -i.cookie_count))


# ----------------------------------------------------------------------------
# Saved credential metadata (sites + usernames only, never passwords)
# ----------------------------------------------------------------------------

@dataclass
class SavedLogin:
    origin: str
    username: str
    times_used: int | None = None


def analyze_saved_logins(profile: Profile, tmpdir: Path) -> list[SavedLogin]:
    logins: list[SavedLogin] = []
    if profile.family == "chromium":
        db = _open_locked_sqlite(profile.path / "Login Data", tmpdir)
        if db is None:
            return []
        try:
            rows = db.execute(
                "SELECT origin_url, username_value, times_used FROM logins"
            ).fetchall()
            for origin, user, used in rows:
                logins.append(SavedLogin(origin or "", user or "", used))
        except sqlite3.Error:
            pass
        db.close()
    else:
        # Firefox: logins.json lists sites; passwords are encrypted (key4.db).
        lj = profile.path / "logins.json"
        if lj.exists():
            try:
                data = json.loads(lj.read_text())
                for entry in data.get("logins", []):
                    logins.append(SavedLogin(
                        entry.get("hostname", ""),
                        "(encrypted)",  # username is also encrypted in Firefox
                        entry.get("timesUsed"),
                    ))
            except (OSError, json.JSONDecodeError):
                pass
    return logins


# ----------------------------------------------------------------------------
# Cache usage
# ----------------------------------------------------------------------------

@dataclass
class CacheInfo:
    path: Path | None
    size_bytes: int


def analyze_cache(profile: Profile) -> CacheInfo:
    if profile.cache_path and profile.cache_path.exists():
        return CacheInfo(profile.cache_path, _dir_size(profile.cache_path))
    return CacheInfo(None, 0)


# ----------------------------------------------------------------------------
# Report assembly
# ----------------------------------------------------------------------------

@dataclass
class ProfileReport:
    profile: Profile
    cookies: list[DomainCookieInfo]
    logins: list[SavedLogin]
    cache: CacheInfo

    def to_dict(self) -> dict:
        return {
            "browser": self.profile.browser,
            "profile": self.profile.name,
            "path": str(self.profile.path),
            "logged_in_sites": [
                {"domain": c.domain, "cookies": c.cookie_count,
                 "signals": sorted(c.matched_names & (KNOWN_LOGIN_COOKIES | set(SESSION_HINTS)))}
                for c in self.cookies if c.likely_logged_in
            ],
            "other_cookie_domains": [
                {"domain": c.domain, "cookies": c.cookie_count}
                for c in self.cookies if not c.likely_logged_in
            ],
            "saved_credential_sites": [
                {"origin": l.origin, "username": l.username, "times_used": l.times_used}
                for l in self.logins
            ],
            "cache": {
                "path": str(self.cache.path) if self.cache.path else None,
                "size_bytes": self.cache.size_bytes,
            },
        }


def build_report(profile: Profile, tmpdir: Path) -> ProfileReport:
    return ProfileReport(
        profile=profile,
        cookies=analyze_cookies(profile, tmpdir),
        logins=analyze_saved_logins(profile, tmpdir),
        cache=analyze_cache(profile),
    )


# ----------------------------------------------------------------------------
# Terminal rendering
# ----------------------------------------------------------------------------

class C:
    """ANSI colors, disabled when not a TTY or NO_COLOR is set."""
    _on = sys.stdout.isatty() and "NO_COLOR" not in os.environ
    BOLD = "\033[1m" if _on else ""
    DIM = "\033[2m" if _on else ""
    GREEN = "\033[32m" if _on else ""
    YELLOW = "\033[33m" if _on else ""
    CYAN = "\033[36m" if _on else ""
    RED = "\033[31m" if _on else ""
    RESET = "\033[0m" if _on else ""


def render_report(rep: ProfileReport, show_all_cookies: bool, top: int) -> None:
    p = rep.profile
    print(f"\n{C.BOLD}{C.CYAN}▌ {p.browser} — profile “{p.name}”{C.RESET}")
    print(f"{C.DIM}  {p.path}{C.RESET}")

    logged_in = [c for c in rep.cookies if c.likely_logged_in]
    other = [c for c in rep.cookies if not c.likely_logged_in]

    # Logged-in sites
    print(f"\n  {C.BOLD}Likely logged-in sites{C.RESET} "
          f"{C.DIM}({len(logged_in)}){C.RESET}")
    if logged_in:
        for c in logged_in[:top]:
            signals = ", ".join(sorted(c.matched_names & (KNOWN_LOGIN_COOKIES | set(SESSION_HINTS)))[:4])
            print(f"    {C.GREEN}●{C.RESET} {c.domain:<32} "
                  f"{C.DIM}{c.cookie_count} cookies{C.RESET}"
                  + (f"  {C.DIM}[{signals}]{C.RESET}" if signals else ""))
        if len(logged_in) > top:
            print(f"    {C.DIM}… and {len(logged_in) - top} more{C.RESET}")
    else:
        print(f"    {C.DIM}none detected{C.RESET}")

    # Other cookie domains
    print(f"\n  {C.BOLD}Other domains with cookies{C.RESET} "
          f"{C.DIM}({len(other)}){C.RESET}")
    if show_all_cookies:
        for c in other:
            print(f"    {C.DIM}○ {c.domain:<32} {c.cookie_count} cookies{C.RESET}")
    else:
        preview = ", ".join(c.domain for c in other[:8])
        more = f" … +{len(other) - 8} more" if len(other) > 8 else ""
        print(f"    {C.DIM}{preview}{more}{C.RESET}")

    # Saved credentials
    print(f"\n  {C.BOLD}Saved-password sites{C.RESET} "
          f"{C.DIM}({len(rep.logins)}){C.RESET}")
    if rep.logins:
        seen = set()
        shown = 0
        for l in rep.logins:
            key = (l.origin, l.username)
            if key in seen:
                continue
            seen.add(key)
            if shown >= top:
                break
            user = f" {C.DIM}({l.username}){C.RESET}" if l.username and l.username != "(encrypted)" else ""
            print(f"    {C.YELLOW}🔑{C.RESET} {l.origin}{user}")
            shown += 1
        if len(seen) > top:
            print(f"    {C.DIM}… and {len(seen) - top} more{C.RESET}")
    else:
        print(f"    {C.DIM}none saved (or password store uses OS keyring only){C.RESET}")

    # Cache
    print(f"\n  {C.BOLD}HTTP cache{C.RESET}")
    if rep.cache.path:
        print(f"    {human_size(rep.cache.size_bytes)}  {C.DIM}{rep.cache.path}{C.RESET}")
    else:
        print(f"    {C.DIM}not found (browser may store it elsewhere){C.RESET}")


def render_summary(reports: list[ProfileReport]) -> None:
    total_logins = sum(len([c for c in r.cookies if c.likely_logged_in]) for r in reports)
    total_saved = sum(len(r.logins) for r in reports)
    total_cache = sum(r.cache.size_bytes for r in reports)
    print(f"\n{C.BOLD}Summary{C.RESET}")
    print(f"  Profiles scanned : {len(reports)}")
    print(f"  Logged-in sites  : {total_logins}")
    print(f"  Saved-password   : {total_saved}")
    print(f"  Total cache      : {human_size(total_cache)}")


# ----------------------------------------------------------------------------
# CLI
# ----------------------------------------------------------------------------

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        prog="aletheia",
        description="Audit YOUR OWN browser profiles: logged-in sites, "
                    "saved-credential sites, and cache usage.",
    )
    ap.add_argument("--json", action="store_true",
                    help="Emit machine-readable JSON instead of a report.")
    ap.add_argument("--all-cookies", action="store_true",
                    help="List every cookie domain, not just a preview.")
    ap.add_argument("--top", type=int, default=25,
                    help="Max rows per section in the terminal report (default 25).")
    ap.add_argument("--browser", action="append", default=None,
                    help="Limit to a browser by name (repeatable), e.g. --browser Brave.")
    ap.add_argument("--list", action="store_true",
                    help="Only list discovered profiles, then exit.")
    args = ap.parse_args(argv)

    profiles = discover_profiles()
    if args.browser:
        wanted = {b.lower() for b in args.browser}
        profiles = [p for p in profiles if p.browser.lower() in wanted]

    if not profiles:
        print("No browser profiles found for the current user.", file=sys.stderr)
        return 1

    if args.list:
        for p in profiles:
            print(f"{p.browser:<10} {p.name:<24} {p.path}")
        return 0

    with tempfile.TemporaryDirectory(prefix="cacheextractor_") as td:
        tmpdir = Path(td)
        reports = [build_report(p, tmpdir) for p in profiles]

    if args.json:
        print(json.dumps({
            "generated_at": time.strftime("%Y-%m-%dT%H:%M:%S"),
            "profiles": [r.to_dict() for r in reports],
        }, indent=2))
        return 0

    print(f"{C.BOLD}Aletheia{C.RESET} {C.DIM}— local browser privacy audit"
          f" ({time.strftime('%Y-%m-%d %H:%M')}){C.RESET}")
    print(f"{C.DIM}Reads only your own profiles. Never decrypts or shows "
          f"passwords/cookie values.{C.RESET}")
    for rep in reports:
        render_report(rep, args.all_cookies, args.top)
    render_summary(reports)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
