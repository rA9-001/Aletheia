//! Browser profile scanning: logged-in sites, saved-credential sites, cache use.
//!
//! Ethics/scope (mirrors the original CLI):
//!   * Reads only the LOCAL user's own profiles.
//!   * Never decrypts or returns password / cookie *values* — only which sites
//!     have credentials or session cookies.
//!   * Copies locked SQLite files before reading, so the browser can stay open.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Profile {
    pub browser: String,
    pub family: Family,
    pub name: String,
    pub path: PathBuf,
    pub cache_path: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Chromium,
    Firefox,
}

/// One cookie's metadata (never its value).
#[derive(Serialize)]
pub struct CookieDetail {
    pub name: String,
    /// Unix seconds when it expires; None = session cookie (cleared on browser exit).
    pub expires_unix: Option<i64>,
    pub secure: bool,
    pub http_only: bool,
}

#[derive(Serialize)]
pub struct DomainCookie {
    pub domain: String,
    pub cookie_count: u32,
    pub signals: Vec<String>,
    /// Per-cookie metadata (names/flags only — never values).
    pub cookies: Vec<CookieDetail>,
    pub likely_logged_in: bool,
    /// Domain matches a known third-party/advertising tracker.
    pub is_tracker: bool,
    /// Most recent time any cookie for this domain was accessed (unix seconds).
    pub last_access_unix: Option<i64>,
}

#[derive(Serialize)]
pub struct SavedLogin {
    pub origin: String,
    pub username: String,
    pub times_used: Option<i64>,
}

#[derive(Serialize)]
pub struct CacheInfo {
    pub path: Option<String>,
    pub size_bytes: u64,
}

#[derive(Serialize)]
pub struct ProfileReport {
    pub browser: String,
    pub family: String,
    pub profile: String,
    pub path: String,
    /// True if the owning browser appears to be running right now. Deleting
    /// cookies/passwords while it runs may not stick (the browser can rewrite
    /// them on exit), so the UI warns about this.
    pub browser_running: bool,
    pub logged_in_sites: Vec<DomainCookie>,
    pub other_cookie_domains: Vec<DomainCookie>,
    pub saved_credential_sites: Vec<SavedLogin>,
    pub cache: CacheInfo,
}

#[derive(Serialize)]
pub struct ScanResult {
    pub generated_at: u64,
    pub profiles: Vec<ProfileReport>,
}

// ---------------------------------------------------------------------------
// Heuristics
// ---------------------------------------------------------------------------

// Substrings that suggest an authenticated session. Kept deliberately specific
// to avoid false positives (e.g. bare "sid" matched "sidebar"); "session" still
// catches "sessionid", "sessionkey", "wikia_session_id", etc.
const SESSION_HINTS: &[&str] = &[
    "session",
    "auth",
    "token",
    "login",
    "logged",
    "jwt",
    "sso",
    "remember",
    "__host-",
    "__secure-",
];

const KNOWN_LOGIN_COOKIES: &[&str] = &[
    "li_at",
    "sessionid",
    "phpsessid",
    "jsessionid",
    "__secure-1psid",
    "sapisid",
    "c_user",
    "xs",
    "ds_user_id",
    "reddit_session",
    "twid",
    "auth_token",
    "user_session",
    "_gh_sess",
];

// Registrable domains of well-known third-party trackers / ad networks.
const TRACKER_DOMAINS: &[&str] = &[
    "doubleclick.net",
    "google-analytics.com",
    "googletagmanager.com",
    "googlesyndication.com",
    "googleadservices.com",
    "adnxs.com",
    "scorecardresearch.com",
    "quantserve.com",
    "criteo.com",
    "criteo.net",
    "taboola.com",
    "outbrain.com",
    "hotjar.com",
    "segment.com",
    "segment.io",
    "mixpanel.com",
    "amplitude.com",
    "branch.io",
    "adsrvr.org",
    "rubiconproject.com",
    "pubmatic.com",
    "openx.net",
    "casalemedia.com",
    "bluekai.com",
    "demdex.net",
    "everesttech.net",
    "mathtag.com",
    "bidswitch.net",
    "yieldmo.com",
    "facebook.com",
    "fbcdn.net",
    "ads-twitter.com",
    "bing.com",
    "clarity.ms",
    "newrelic.com",
    "nr-data.net",
    "optimizely.com",
    "fullstory.com",
    "cloudflareinsights.com",
    "sentry.io",
    "chartbeat.com",
    "sharethis.com",
    "addthis.com",
    "onetrust.com",
    "cookielaw.org",
];

fn is_tracker_domain(domain: &str) -> bool {
    TRACKER_DOMAINS.contains(&domain)
}

/// Cheap eTLD+1 approximation (no public-suffix list dependency).
pub(crate) fn registrable_domain(host: &str) -> String {
    let host = host.trim_start_matches('.');
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() <= 2 {
        return host.to_string();
    }
    let two_level = ["co", "com", "org", "net", "gov", "edu", "ac"];
    let n = parts.len();
    if n >= 3 && two_level.contains(&parts[n - 2]) && parts[n - 1].len() == 2 {
        return parts[n - 3..].join(".");
    }
    parts[n - 2..].join(".")
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// (browser display name, config root) for Chromium-family browsers.
fn chromium_roots() -> Vec<(String, PathBuf)> {
    let h = home();
    let mut candidates: Vec<(&str, PathBuf)> = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let base = h.join(".config");
        candidates.push(("Brave", base.join("BraveSoftware/Brave-Browser")));
        candidates.push(("Chrome", base.join("google-chrome")));
        candidates.push(("Chromium", base.join("chromium")));
        candidates.push(("Vivaldi", base.join("vivaldi")));
        candidates.push(("Edge", base.join("microsoft-edge")));
    }
    #[cfg(target_os = "macos")]
    {
        let base = h.join("Library/Application Support");
        candidates.push(("Brave", base.join("BraveSoftware/Brave-Browser")));
        candidates.push(("Chrome", base.join("Google/Chrome")));
        candidates.push(("Chromium", base.join("Chromium")));
        candidates.push(("Edge", base.join("Microsoft Edge")));
    }
    #[cfg(target_os = "windows")]
    {
        let base = dirs::data_local_dir().unwrap_or_else(|| h.join("AppData/Local"));
        candidates.push(("Brave", base.join("BraveSoftware/Brave-Browser/User Data")));
        candidates.push(("Chrome", base.join("Google/Chrome/User Data")));
        candidates.push(("Chromium", base.join("Chromium/User Data")));
        candidates.push(("Edge", base.join("Microsoft Edge/User Data")));
    }

    candidates
        .into_iter()
        .filter(|(_, p)| p.is_dir())
        .map(|(n, p)| (n.to_string(), p))
        .collect()
}

fn firefox_roots() -> Vec<(String, PathBuf)> {
    let h = home();
    let mut out: Vec<(String, PathBuf)> = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let ff = h.join(".mozilla/firefox");
        if ff.is_dir() {
            out.push(("Firefox".into(), ff));
        }
        let lw = h.join(".librewolf");
        if lw.is_dir() {
            out.push(("LibreWolf".into(), lw));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let base = h.join("Library/Application Support");
        for (name, rel) in [("Firefox", "Firefox"), ("LibreWolf", "LibreWolf")] {
            let p = base.join(rel);
            if p.is_dir() {
                out.push((name.into(), p));
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        let base = dirs::data_dir().unwrap_or_else(|| h.join("AppData/Roaming"));
        let ff = base.join("Mozilla/Firefox");
        if ff.is_dir() {
            out.push(("Firefox".into(), ff));
        }
        let lw = base.join("librewolf");
        if lw.is_dir() {
            out.push(("LibreWolf".into(), lw));
        }
    }

    out
}

pub fn discover_profiles() -> Vec<Profile> {
    let mut profiles = Vec::new();

    // Chromium family: profile dirs contain a "Cookies" or "Preferences" file.
    for (browser, root) in chromium_roots() {
        if let Ok(entries) = fs::read_dir(&root) {
            let mut dirs_: Vec<PathBuf> =
                entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
            dirs_.sort();
            for child in dirs_ {
                if !child.is_dir() {
                    continue;
                }
                if !child.join("Preferences").exists() && !child.join("Cookies").exists() {
                    continue;
                }
                let name = child.file_name().unwrap().to_string_lossy().to_string();
                let cache = chromium_cache_dir(&root, &name);
                profiles.push(Profile {
                    browser: browser.clone(),
                    family: Family::Chromium,
                    name,
                    path: child,
                    cache_path: cache,
                });
            }
        }
    }

    // Firefox family: parse profiles.ini, else glob.
    for (browser, root) in firefox_roots() {
        let prof_dirs = firefox_profile_dirs(&root);
        for p in prof_dirs {
            if !p.is_dir() {
                continue;
            }
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let cache = firefox_cache_dir(&root, &p);
            profiles.push(Profile {
                browser: browser.clone(),
                family: Family::Firefox,
                name,
                path: p,
                cache_path: cache,
            });
        }
    }

    profiles
}

fn firefox_profile_dirs(root: &Path) -> Vec<PathBuf> {
    let ini = root.join("profiles.ini");
    let mut out = Vec::new();
    if let Ok(text) = fs::read_to_string(&ini) {
        let mut cur_path: Option<String> = None;
        let mut cur_rel = true;
        let flush = |out: &mut Vec<PathBuf>, path: &Option<String>, rel: bool| {
            if let Some(rp) = path {
                if rel {
                    out.push(root.join(rp));
                } else {
                    out.push(PathBuf::from(rp));
                }
            }
        };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                flush(&mut out, &cur_path, cur_rel);
                cur_path = None;
                cur_rel = true;
            } else if let Some(v) = line.strip_prefix("Path=") {
                cur_path = Some(v.trim().to_string());
            } else if let Some(v) = line.strip_prefix("IsRelative=") {
                cur_rel = v.trim() != "0";
            }
        }
        flush(&mut out, &cur_path, cur_rel);
    }
    if out.is_empty() {
        if let Ok(entries) = fs::read_dir(root) {
            for e in entries.filter_map(|e| e.ok()) {
                let p = e.path();
                if p.is_dir()
                    && p.file_name()
                        .is_some_and(|n| n.to_string_lossy().contains('.'))
                {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn chromium_cache_dir(root: &Path, profile_name: &str) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        // ~/.config/X -> ~/.cache/X/<profile>/Cache
        if let Ok(rel) = root.strip_prefix(home().join(".config")) {
            let cand = home()
                .join(".cache")
                .join(rel)
                .join(profile_name)
                .join("Cache");
            if cand.exists() {
                return Some(cand);
            }
        }
    }
    let cand = root.join(profile_name).join("Cache");
    cand.exists().then_some(cand)
}

fn firefox_cache_dir(root: &Path, profile_dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        let pname = profile_dir.file_name()?.to_string_lossy().to_string();
        if let Ok(rel) = root.strip_prefix(home()) {
            // ".mozilla/firefox" -> "mozilla/firefox" ; ".librewolf" -> "librewolf"
            let comps: Vec<String> = rel
                .components()
                .map(|c| {
                    c.as_os_str()
                        .to_string_lossy()
                        .trim_start_matches('.')
                        .to_string()
                })
                .collect();
            let mut bases = Vec::new();
            if !comps.is_empty() {
                let mut b = home().join(".cache");
                for c in &comps {
                    b = b.join(c);
                }
                bases.push(b);
                bases.push(home().join(".cache").join(comps.last().unwrap()));
            }
            for base in bases {
                let cand = base.join(&pname).join("cache2");
                if cand.exists() {
                    return Some(cand);
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        // Profiles live under %APPDATA% (Roaming) but the cache lives at the same
        // relative path under %LOCALAPPDATA% (Local), e.g.
        //   %APPDATA%\Mozilla\Firefox\Profiles\<p>
        //   -> %LOCALAPPDATA%\Mozilla\Firefox\Profiles\<p>\cache2
        let _ = root;
        if let (Some(roaming), Some(local)) = (dirs::data_dir(), dirs::data_local_dir()) {
            if let Ok(rel) = profile_dir.strip_prefix(&roaming) {
                let cand = local.join(rel).join("cache2");
                if cand.exists() {
                    return Some(cand);
                }
            }
        }
    }
    let cand = profile_dir.join("cache2");
    cand.exists().then_some(cand)
}

// ---------------------------------------------------------------------------
// SQLite helpers
// ---------------------------------------------------------------------------

/// Copy a possibly-locked SQLite DB (and its WAL/SHM) to a temp file, open RO.
fn open_locked_sqlite(db_path: &Path) -> Option<Connection> {
    if !db_path.exists() {
        return None;
    }
    let mut tmp = std::env::temp_dir();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    let uniq = format!(
        "cacheextractor_{}_{}",
        stamp,
        db_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    );
    tmp.push(uniq);
    if fs::copy(db_path, &tmp).is_ok() {
        for suffix in ["-wal", "-shm"] {
            let side = PathBuf::from(format!("{}{}", db_path.display(), suffix));
            if side.exists() {
                let dst = PathBuf::from(format!("{}{}", tmp.display(), suffix));
                let _ = fs::copy(&side, &dst);
            }
        }
        if let Ok(conn) = Connection::open_with_flags(&tmp, OpenFlags::SQLITE_OPEN_READ_ONLY) {
            return Some(conn);
        }
    }
    // Last resort: open the original read-only.
    Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .ok()
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Ok(entries) = fs::read_dir(&p) {
            for e in entries.filter_map(|e| e.ok()) {
                match e.file_type() {
                    Ok(ft) if ft.is_dir() => stack.push(e.path()),
                    Ok(ft) if ft.is_file() => {
                        if let Ok(md) = e.metadata() {
                            total += md.len();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    total
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

struct DomAcc {
    count: u32,
    score: u32,
    matched: Vec<String>,
    cookies: Vec<CookieDetail>,
    known_hit: bool,
    last_access: Option<i64>,
}

/// Chromium timestamps are microseconds since 1601-01-01; Firefox uses either
/// unix seconds (expiry) or unix microseconds (lastAccessed). Normalize to unix
/// seconds, returning None for zero/absent.
fn chromium_micros_to_unix(v: i64) -> Option<i64> {
    if v <= 0 {
        None
    } else {
        Some(v / 1_000_000 - 11_644_473_600)
    }
}

fn analyze_cookies(profile: &Profile) -> Vec<DomainCookie> {
    let (db_file, table, sql) = match profile.family {
        Family::Chromium => (
            "Cookies",
            "cookies",
            "SELECT name, host_key, expires_utc, last_access_utc, is_secure, is_httponly FROM cookies",
        ),
        Family::Firefox => (
            "cookies.sqlite",
            "moz_cookies",
            "SELECT name, host, expiry, lastAccessed, isSecure, isHttpOnly FROM moz_cookies",
        ),
    };
    let _ = table;
    let conn = match open_locked_sqlite(&profile.path.join(db_file)) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let chromium = matches!(profile.family, Family::Chromium);
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?.unwrap_or_default(), // name
            r.get::<_, Option<String>>(1)?.unwrap_or_default(), // host
            r.get::<_, Option<i64>>(2)?.unwrap_or(0),           // expiry
            r.get::<_, Option<i64>>(3)?.unwrap_or(0),           // last access
            r.get::<_, Option<i64>>(4)?.unwrap_or(0) != 0,      // secure
            r.get::<_, Option<i64>>(5)?.unwrap_or(0) != 0,      // http_only
        ))
    });
    let rows = match rows {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    let mut acc: BTreeMap<String, DomAcc> = BTreeMap::new();
    for (name, host, expiry_raw, access_raw, secure, http_only) in rows.flatten() {
        if host.is_empty() {
            continue;
        }
        let dom = registrable_domain(&host);
        let entry = acc.entry(dom).or_insert(DomAcc {
            count: 0,
            score: 0,
            matched: Vec::new(),
            cookies: Vec::new(),
            known_hit: false,
            last_access: None,
        });
        entry.count += 1;

        let (expires_unix, access_unix) = if chromium {
            (
                chromium_micros_to_unix(expiry_raw),
                chromium_micros_to_unix(access_raw),
            )
        } else {
            // Firefox: expiry is unix seconds, lastAccessed is unix microseconds.
            (
                (expiry_raw > 0).then_some(expiry_raw),
                (access_raw > 0).then_some(access_raw / 1_000_000),
            )
        };
        if let Some(a) = access_unix {
            entry.last_access = Some(entry.last_access.map_or(a, |cur| cur.max(a)));
        }

        let lname = name.to_lowercase();
        entry.cookies.push(CookieDetail {
            name: lname.clone(),
            expires_unix,
            secure,
            http_only,
        });

        for hint in SESSION_HINTS {
            if lname.contains(hint) {
                entry.score += 1;
                if !entry.matched.contains(&lname) {
                    entry.matched.push(lname.clone());
                }
                break;
            }
        }
        if KNOWN_LOGIN_COOKIES.contains(&lname.as_str()) {
            entry.score += 2;
            entry.known_hit = true;
        }
    }

    let mut out: Vec<DomainCookie> = acc
        .into_iter()
        .map(|(domain, a)| {
            let likely = a.score >= 2 || a.known_hit;
            let mut signals: Vec<String> = a.matched;
            signals.sort();
            signals.dedup();
            let mut cookies = a.cookies;
            cookies.sort_by(|x, y| x.name.cmp(&y.name));
            let is_tracker = is_tracker_domain(&domain);
            DomainCookie {
                domain,
                cookie_count: a.count,
                signals,
                cookies,
                likely_logged_in: likely,
                is_tracker,
                last_access_unix: a.last_access,
            }
        })
        .collect();

    out.sort_by(|a, b| {
        (b.likely_logged_in as u8)
            .cmp(&(a.likely_logged_in as u8))
            .then(b.cookie_count.cmp(&a.cookie_count))
    });
    out
}

fn analyze_saved_logins(profile: &Profile) -> Vec<SavedLogin> {
    match profile.family {
        Family::Chromium => {
            let conn = match open_locked_sqlite(&profile.path.join("Login Data")) {
                Some(c) => c,
                None => return Vec::new(),
            };
            let mut stmt =
                match conn.prepare("SELECT origin_url, username_value, times_used FROM logins") {
                    Ok(s) => s,
                    Err(_) => return Vec::new(),
                };
            let rows = stmt.query_map([], |r| {
                Ok(SavedLogin {
                    origin: r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    username: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    times_used: r.get::<_, Option<i64>>(2)?,
                })
            });
            match rows {
                Ok(r) => r.flatten().collect(),
                Err(_) => Vec::new(),
            }
        }
        Family::Firefox => {
            // logins.json lists sites; usernames/passwords are encrypted.
            let lj = profile.path.join("logins.json");
            let text = match fs::read_to_string(&lj) {
                Ok(t) => t,
                Err(_) => return Vec::new(),
            };
            let json: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => return Vec::new(),
            };
            json.get("logins")
                .and_then(|l| l.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|e| SavedLogin {
                            origin: e
                                .get("hostname")
                                .and_then(|h| h.as_str())
                                .unwrap_or("")
                                .to_string(),
                            username: "(encrypted)".to_string(),
                            times_used: e.get("timesUsed").and_then(|t| t.as_i64()),
                        })
                        .collect()
                })
                .unwrap_or_default()
        }
    }
}

fn analyze_cache(profile: &Profile) -> CacheInfo {
    match &profile.cache_path {
        Some(p) if p.exists() => CacheInfo {
            path: Some(p.display().to_string()),
            size_bytes: dir_size(p),
        },
        _ => CacheInfo {
            path: None,
            size_bytes: 0,
        },
    }
}

fn build_report(profile: &Profile) -> ProfileReport {
    let cookies = analyze_cookies(profile);
    let (logged_in, other): (Vec<_>, Vec<_>) =
        cookies.into_iter().partition(|c| c.likely_logged_in);
    ProfileReport {
        browser: profile.browser.clone(),
        family: match profile.family {
            Family::Chromium => "chromium".into(),
            Family::Firefox => "firefox".into(),
        },
        profile: profile.name.clone(),
        path: profile.path.display().to_string(),
        browser_running: is_browser_running(&profile.browser),
        logged_in_sites: logged_in,
        other_cookie_domains: other,
        saved_credential_sites: analyze_saved_logins(profile),
        cache: analyze_cache(profile),
    }
}

/// Entry point used by the Tauri command.
pub fn scan_all() -> ScanResult {
    let generated_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let profiles = discover_profiles()
        .iter()
        .map(build_report)
        .collect::<Vec<_>>();
    ScanResult {
        generated_at,
        profiles,
    }
}

// ---------------------------------------------------------------------------
// Running-browser detection
// ---------------------------------------------------------------------------

fn browser_needle(browser: &str) -> &'static str {
    match browser {
        "Brave" => "brave",
        "Chrome" => "chrome",
        "Chromium" => "chromium",
        "Vivaldi" => "vivaldi",
        "Edge" => "edge",
        "Firefox" => "firefox",
        "LibreWolf" => "librewolf",
        _ => "",
    }
}

/// Best-effort check whether the given browser is currently running.
/// Linux scans process names in /proc; Windows queries `tasklist`.
pub fn is_browser_running(browser: &str) -> bool {
    let needle = browser_needle(browser);
    if needle.is_empty() {
        return false;
    }

    #[cfg(target_os = "linux")]
    {
        let Ok(entries) = fs::read_dir("/proc") else {
            return false;
        };
        for e in entries.filter_map(|e| e.ok()) {
            // Only numeric PID directories.
            if !e
                .file_name()
                .to_string_lossy()
                .chars()
                .all(|c| c.is_ascii_digit())
            {
                continue;
            }
            if let Ok(comm) = fs::read_to_string(e.path().join("comm")) {
                if comm.trim().to_lowercase().contains(needle) {
                    return true;
                }
            }
        }
        false
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000; // don't flash a console window
        match std::process::Command::new("tasklist")
            .args(["/FO", "CSV", "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout)
                .to_lowercase()
                .contains(needle),
            Err(_) => false,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

// ---------------------------------------------------------------------------
// Deletion (modifies the real profile databases)
// ---------------------------------------------------------------------------

fn open_rw(db_path: &Path) -> Result<Connection, String> {
    if !db_path.exists() {
        return Err(format!("database not found: {}", db_path.display()));
    }
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_WRITE)
        .map_err(|e| format!("cannot open {} for writing: {e}", db_path.display()))?;
    // Wait rather than fail immediately if the browser briefly holds a lock.
    let _ = conn.busy_timeout(std::time::Duration::from_millis(4000));
    Ok(conn)
}

/// Delete every cookie belonging to `domain` (registrable domain) for a profile.
/// Returns the number of cookie rows removed.
pub fn delete_cookies_for_domain(
    profile_path: &str,
    family: &str,
    domain: &str,
) -> Result<usize, String> {
    let base = Path::new(profile_path);
    let (db_file, host_col, table) = match family {
        "chromium" => ("Cookies", "host_key", "cookies"),
        "firefox" => ("cookies.sqlite", "host", "moz_cookies"),
        other => return Err(format!("unknown browser family: {other}")),
    };
    let conn = open_rw(&base.join(db_file))?;

    // Find the exact host_key values whose registrable domain matches.
    let hosts: Vec<String> = {
        let mut stmt = conn
            .prepare(&format!("SELECT DISTINCT {host_col} FROM {table}"))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, Option<String>>(0))
            .map_err(|e| e.to_string())?;
        rows.flatten()
            .flatten()
            .filter(|h| !h.is_empty() && registrable_domain(h) == domain)
            .collect()
    };
    if hosts.is_empty() {
        return Ok(0);
    }

    let mut deleted = 0usize;
    for h in &hosts {
        deleted += conn
            .execute(&format!("DELETE FROM {table} WHERE {host_col} = ?1"), [h])
            .map_err(|e| e.to_string())?;
    }
    Ok(deleted)
}

/// Delete all cookies for any of the given registrable domains in one pass.
pub fn delete_cookies_bulk(
    profile_path: &str,
    family: &str,
    domains: &[String],
) -> Result<usize, String> {
    if domains.is_empty() {
        return Ok(0);
    }
    let want: std::collections::HashSet<&str> = domains.iter().map(|s| s.as_str()).collect();
    let base = Path::new(profile_path);
    let (db_file, host_col, table) = match family {
        "chromium" => ("Cookies", "host_key", "cookies"),
        "firefox" => ("cookies.sqlite", "host", "moz_cookies"),
        other => return Err(format!("unknown browser family: {other}")),
    };
    let conn = open_rw(&base.join(db_file))?;

    let hosts: Vec<String> = {
        let mut stmt = conn
            .prepare(&format!("SELECT DISTINCT {host_col} FROM {table}"))
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| r.get::<_, Option<String>>(0))
            .map_err(|e| e.to_string())?;
        rows.flatten()
            .flatten()
            .filter(|h| !h.is_empty() && want.contains(registrable_domain(h).as_str()))
            .collect()
    };

    let mut deleted = 0usize;
    for h in &hosts {
        deleted += conn
            .execute(&format!("DELETE FROM {table} WHERE {host_col} = ?1"), [h])
            .map_err(|e| e.to_string())?;
    }
    Ok(deleted)
}

/// Empty a profile's HTTP cache directory. Validates the path is a real browser
/// cache dir under the user's home before deleting anything. Returns bytes freed.
pub fn clear_cache(cache_path: &str) -> Result<u64, String> {
    let path = Path::new(cache_path);
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    // Guard: only ever touch a directory literally named Cache/cache2, inside home.
    if !(name == "Cache" || name == "cache2") {
        return Err("refusing: not a recognized cache directory".into());
    }
    let home = dirs::home_dir().ok_or("cannot resolve home directory")?;
    if !path.starts_with(&home) {
        return Err("refusing: cache path is outside your home directory".into());
    }
    if !path.is_dir() {
        return Err("cache directory not found".into());
    }
    let freed = dir_size(path);
    for entry in fs::read_dir(path).map_err(|e| e.to_string())?.flatten() {
        let p = entry.path();
        let res = if p.is_dir() {
            fs::remove_dir_all(&p)
        } else {
            fs::remove_file(&p)
        };
        // Ignore individual files the browser holds open; report the rest freed.
        let _ = res;
    }
    Ok(freed)
}

/// Delete a saved password entry. For Chromium it matches origin + username; for
/// Firefox (values encrypted) it removes all logins for the origin's hostname.
pub fn delete_saved_password(
    profile_path: &str,
    family: &str,
    origin: &str,
    username: &str,
) -> Result<usize, String> {
    let base = Path::new(profile_path);
    match family {
        "chromium" => {
            let conn = open_rw(&base.join("Login Data"))?;
            let n = conn
                .execute(
                    "DELETE FROM logins WHERE origin_url = ?1 AND username_value = ?2",
                    rusqlite::params![origin, username],
                )
                .map_err(|e| e.to_string())?;
            Ok(n)
        }
        "firefox" => {
            let lj = base.join("logins.json");
            let text = fs::read_to_string(&lj).map_err(|e| e.to_string())?;
            let mut json: serde_json::Value =
                serde_json::from_str(&text).map_err(|e| e.to_string())?;
            let Some(arr) = json.get_mut("logins").and_then(|l| l.as_array_mut()) else {
                return Ok(0);
            };
            let before = arr.len();
            arr.retain(|e| e.get("hostname").and_then(|h| h.as_str()) != Some(origin));
            let removed = before - arr.len();
            if removed > 0 {
                fs::write(
                    &lj,
                    serde_json::to_string(&json).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            }
            Ok(removed)
        }
        other => Err(format!("unknown browser family: {other}")),
    }
}

#[cfg(test)]
mod delete_tests {
    use super::*;

    #[test]
    fn deletes_cookies_by_registrable_domain() {
        let dir = std::env::temp_dir().join(format!("ce_ck_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let db = dir.join("Cookies");
        let _ = fs::remove_file(&db);
        {
            let c = Connection::open(&db).unwrap();
            c.execute("CREATE TABLE cookies (host_key TEXT, name TEXT)", [])
                .unwrap();
            for (h, n) in [
                (".example.com", "sid"),
                ("www.example.com", "a"),
                ("sub.example.com", "b"),
                ("otherexample.com", "keep"),
                ("google.com", "keep2"),
            ] {
                c.execute(
                    "INSERT INTO cookies VALUES (?1, ?2)",
                    rusqlite::params![h, n],
                )
                .unwrap();
            }
        }
        let n =
            delete_cookies_for_domain(dir.to_str().unwrap(), "chromium", "example.com").unwrap();
        assert_eq!(n, 3, "should delete the three example.com host_keys only");
        let c = Connection::open(&db).unwrap();
        let remaining: i64 = c
            .query_row("SELECT COUNT(*) FROM cookies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 2, "otherexample.com and google.com must survive");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deletes_chromium_password_by_origin_and_user() {
        let dir = std::env::temp_dir().join(format!("ce_pw_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let db = dir.join("Login Data");
        let _ = fs::remove_file(&db);
        {
            let c = Connection::open(&db).unwrap();
            c.execute(
                "CREATE TABLE logins (origin_url TEXT, username_value TEXT)",
                [],
            )
            .unwrap();
            c.execute("INSERT INTO logins VALUES ('https://a.com/', 'alice')", [])
                .unwrap();
            c.execute("INSERT INTO logins VALUES ('https://a.com/', 'bob')", [])
                .unwrap();
        }
        let n = delete_saved_password(dir.to_str().unwrap(), "chromium", "https://a.com/", "alice")
            .unwrap();
        assert_eq!(n, 1);
        let c = Connection::open(&db).unwrap();
        let remaining: i64 = c
            .query_row("SELECT COUNT(*) FROM logins", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 1, "only alice removed, bob stays");
        let _ = fs::remove_dir_all(&dir);
    }
}
