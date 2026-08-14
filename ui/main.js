// Aletheia frontend. Talks to the Rust backend via the Tauri global API.
const invoke = window.__TAURI__?.core?.invoke;

// Inline line-style SVG icons (inherit color via currentColor, size via 1em).
const ICONS = {
  chevron:
    '<svg class="ico chevron" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>',
  trash:
    '<svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/><path d="M10 11v6M14 11v6"/></svg>',
  star:
    '<svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2.5l2.9 5.9 6.6.9-4.8 4.6 1.1 6.5L12 21l-5.9 3.1 1.1-6.5L2.5 9.3l6.6-.9z"/></svg>',
  starFilled:
    '<svg class="ico" viewBox="0 0 24 24" fill="currentColor" stroke="none"><path d="M12 2.5l2.9 5.9 6.6.9-4.8 4.6 1.1 6.5L12 18.9 6.1 21l1.1-6.5L2.5 9.3l6.6-.9z"/></svg>',
  check:
    '<svg class="ico" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"><path d="M5 12.5l4.5 4.5L19 6.5"/></svg>',
};

const STALE_DAYS = 60; // sessions unused longer than this are flagged.
const now = () => Math.floor(Date.now() / 1000);

function relTime(unix) {
  if (!unix) return "unknown";
  const s = now() - unix;
  const past = s >= 0;
  const a = Math.abs(s);
  const day = 86400;
  let out;
  if (a < 3600) out = `${Math.max(1, Math.round(a / 60))}m`;
  else if (a < day) out = `${Math.round(a / 3600)}h`;
  else if (a < 60 * day) out = `${Math.round(a / day)}d`;
  else if (a < 365 * day) out = `${Math.round(a / (30 * day))}mo`;
  else out = `${(a / (365 * day)).toFixed(1)}y`;
  return past ? `${out} ago` : `in ${out}`;
}

function expiryText(expires) {
  if (!expires) return "session";
  return expires < now() ? "expired" : `expires ${relTime(expires)}`;
}

function isStale(d) {
  return d.last_access_unix && now() - d.last_access_unix > STALE_DAYS * 86400;
}

// ---- keep-list (persisted per profile in localStorage) ----------------------

function keepKey(profile) {
  return `keep:${profile.path}`;
}
function getKeep(profile) {
  try {
    return new Set(JSON.parse(localStorage.getItem(keepKey(profile)) || "[]"));
  } catch {
    return new Set();
  }
}
function isKept(profile, domain) {
  return getKeep(profile).has(domain);
}
function toggleKeep(profile, domain) {
  const set = getKeep(profile);
  set.has(domain) ? set.delete(domain) : set.add(domain);
  localStorage.setItem(keepKey(profile), JSON.stringify([...set]));
}

const el = (id) => document.getElementById(id);
const scanBtn = el("scanBtn");
const loading = el("loading");

let state = {
  profiles: [], // non-empty ProfileReport[]
  selected: 0,
  filter: "",
  picked: new Set(), // domains checked for bulk action, current profile
};

// ---- helpers ----------------------------------------------------------------

function humanSize(n) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${v} B` : `${v.toFixed(1)} ${units[i]}`;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}

function dedupeLogins(logins) {
  const seen = new Set();
  const out = [];
  for (const l of logins) {
    const key = `${l.origin}|${l.username}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(l);
  }
  return out;
}

// A profile is "empty" if it has nothing worth showing.
function hasData(p) {
  return (
    p.logged_in_sites.length > 0 ||
    p.other_cookie_domains.length > 0 ||
    dedupeLogins(p.saved_credential_sites).length > 0 ||
    (p.cache && p.cache.size_bytes > 0)
  );
}

// ---- scanning ---------------------------------------------------------------

async function runScan() {
  if (!invoke) {
    showError(
      "Tauri API unavailable (window.__TAURI__.core.invoke is missing). " +
        "Make sure the app was launched as the built binary, not a plain browser."
    );
    return;
  }
  // Remember which profile was selected so a rescan (incl. after a delete)
  // keeps the user in place instead of jumping to the first profile.
  const prevPath = state.profiles[state.selected]?.path;
  loading.hidden = false;
  scanBtn.disabled = true;
  try {
    const timeout = new Promise((_, reject) =>
      setTimeout(() => reject(new Error("timed out after 30s (no response from backend)")), 30000)
    );
    const result = await Promise.race([invoke("scan_profiles"), timeout]);
    state.profiles = (result.profiles || []).filter(hasData);
    const keep = state.profiles.findIndex((p) => p.path === prevPath);
    state.selected = keep >= 0 ? keep : 0;
    state.picked.clear();
    renderAll();
  } catch (e) {
    showError(`Scan failed: ${e && e.message ? e.message : e}`);
  } finally {
    loading.hidden = true;
    scanBtn.disabled = false;
    scanBtn.querySelector(".scan-label").textContent = "Rescan";
  }
}

function showError(msg) {
  el("empty").hidden = true;
  el("report").hidden = false;
  el("sections").innerHTML = `<div class="error-banner">${escapeHtml(msg)}</div>`;
  el("reportTitle").textContent = "Error";
  el("reportPath").textContent = "";
}

// ---- rendering --------------------------------------------------------------

function renderAll() {
  const { profiles } = state;

  const totalLogins = profiles.reduce((a, p) => a + p.logged_in_sites.length, 0);
  const totalSaved = profiles.reduce(
    (a, p) => a + dedupeLogins(p.saved_credential_sites).length,
    0
  );
  const totalCache = profiles.reduce((a, p) => a + (p.cache.size_bytes || 0), 0);
  el("statProfiles").textContent = profiles.length;
  el("statLogins").textContent = totalLogins;
  el("statSaved").textContent = totalSaved;
  el("statCache").textContent = humanSize(totalCache);
  el("stats").hidden = false;

  const list = el("profileList");
  list.innerHTML = "";
  profiles.forEach((p, i) => {
    const li = document.createElement("li");
    li.className = "profile-item" + (i === state.selected ? " active" : "");
    li.innerHTML = `
      <span class="p-browser">${escapeHtml(p.browser)}</span>
      <span class="p-name">${escapeHtml(p.profile)}</span>
      <span class="p-meta">${p.logged_in_sites.length} logins · ${humanSize(p.cache.size_bytes)}</span>
    `;
    li.onclick = () => {
      state.selected = i;
      state.picked.clear();
      renderAll();
    };
    list.appendChild(li);
  });
  el("sidebar").hidden = profiles.length === 0;

  if (profiles.length === 0) {
    el("empty").hidden = false;
    el("report").hidden = true;
    el("empty").querySelector("h2").textContent = "Nothing to show";
    el("empty").querySelector("p").textContent =
      "No browser profiles with saved sessions, credentials, or cache were found for your user account.";
    return;
  }

  el("empty").hidden = true;
  el("report").hidden = false;
  renderProfile(profiles[state.selected]);
}

function matchFilter(text) {
  return !state.filter || text.toLowerCase().includes(state.filter);
}

function renderProfile(p) {
  el("reportTitle").textContent = `${p.browser} — ${p.profile}`;
  el("reportPath").textContent = p.path;

  renderToolbar(p);

  const sections = el("sections");
  sections.innerHTML = "";

  if (p.browser_running) {
    const warn = document.createElement("div");
    warn.className = "warn-banner";
    warn.innerHTML =
      `<strong>${escapeHtml(p.browser)} is running.</strong> ` +
      "You can still remove items, but the browser may rewrite them when it " +
      "closes — quit it first for deletions to stick.";
    sections.appendChild(warn);
  }

  // The two cookie lists are grouped together at the top so cleanup is coherent.
  const loggedIn = p.logged_in_sites.filter((c) => matchFilter(c.domain));
  sections.appendChild(
    buildSection({
      title: "Likely logged-in sites",
      count: p.logged_in_sites.length,
      badgeClass: "green",
      startOpen: true,
      hint: "Sites with active session/login cookies. Click a row to see its cookies, or use the trash icon to remove them.",
      body: siteRows(loggedIn, "green", p),
      emptyText: state.filter ? "No matches." : "No active sessions detected.",
    })
  );

  const other = p.other_cookie_domains.filter((c) => matchFilter(c.domain));
  sections.appendChild(
    buildSection({
      title: "Other domains with cookies",
      count: p.other_cookie_domains.length,
      badgeClass: "",
      startOpen: true,
      hint: "Cookies present, but no clear login signal. Click a row to see its cookies, or use the trash icon to remove them.",
      body: siteRows(other, "dim", p),
      emptyText: state.filter ? "No matches." : "None.",
    })
  );

  const logins = dedupeLogins(p.saved_credential_sites).filter((l) =>
    matchFilter(l.origin || "")
  );
  sections.appendChild(
    buildSection({
      title: "Saved-password sites",
      count: dedupeLogins(p.saved_credential_sites).length,
      badgeClass: "amber",
      startOpen: true,
      hint: "Sites you saved a password for. Passwords themselves are never read.",
      body: loginRows(logins, p),
      emptyText: state.filter ? "No matches." : "No saved passwords (or the browser uses the OS keyring only).",
    })
  );

  sections.appendChild(cacheSection(p.cache));
}

// A collapsible section card.
function buildSection({ title, count, badgeClass, startOpen, hint, body, emptyText }) {
  const sec = document.createElement("section");
  sec.className = "panel" + (startOpen ? "" : " collapsed");

  const head = document.createElement("button");
  head.className = "panel-head";
  head.type = "button";
  head.innerHTML = `
    ${ICONS.chevron}
    <span class="panel-title">${escapeHtml(title)}</span>
    <span class="badge ${badgeClass}">${count}</span>
  `;
  head.onclick = () => sec.classList.toggle("collapsed");
  sec.appendChild(head);

  const wrap = document.createElement("div");
  wrap.className = "panel-body";
  if (hint) {
    const h = document.createElement("p");
    h.className = "panel-hint";
    h.textContent = hint;
    wrap.appendChild(h);
  }
  if (body && body.childElementCount > 0) {
    wrap.appendChild(body);
  } else {
    const e = document.createElement("div");
    e.className = "panel-empty";
    e.textContent = emptyText;
    wrap.appendChild(e);
  }
  sec.appendChild(wrap);
  return sec;
}

// Expandable rows: header shows controls + domain + pills + count; body lists
// the cookies with their flags and expiry.
function siteRows(domains, dotClass, profile) {
  const frag = document.createDocumentFragment();
  for (const d of domains) {
    const kept = isKept(profile, d.domain);
    const row = document.createElement("div");
    row.className = "site collapsed" + (kept ? " kept" : "");

    const pills = [];
    if (d.likely_logged_in) pills.push(`<span class="pill green">logged in</span>`);
    if (d.is_tracker) pills.push(`<span class="pill purple">tracker</span>`);
    if (isStale(d)) pills.push(`<span class="pill amber">unused ${escapeHtml(relTime(d.last_access_unix))}</span>`);

    const head = document.createElement("div");
    head.className = "site-head";
    head.innerHTML = `
      <label class="pick-zone" title="${kept ? "Kept sites can't be bulk-selected" : "Select for bulk action"}">
        <input type="checkbox" class="pick" ${state.picked.has(d.domain) ? "checked" : ""} ${kept ? "disabled" : ""} />
        <span class="checkbox">${ICONS.check}</span>
      </label>
      <span class="dot ${dotClass}"></span>
      <span class="site-main">
        <span class="site-domain">${escapeHtml(d.domain)}</span>
        ${pills.join("")}
      </span>
      <span class="site-count">${d.cookie_count} cookie${d.cookie_count === 1 ? "" : "s"}</span>
      <button class="icon-btn keep ${kept ? "on" : ""}" type="button" title="${kept ? "Kept — protected from bulk clear" : "Keep (protect from bulk clear)"}">${kept ? ICONS.starFilled : ICONS.star}</button>
      <button class="icon-btn danger" type="button" title="Remove all cookies for ${escapeHtml(d.domain)}">${ICONS.trash}</button>
      <span class="chevron-wrap">${ICONS.chevron}</span>
    `;
    // Expand only when clicking the non-interactive parts of the header
    // (not the checkbox zone or the action buttons).
    head.addEventListener("click", (ev) => {
      if (ev.target.closest("button, .pick-zone")) return;
      row.classList.toggle("collapsed");
    });
    head.querySelector(".pick").addEventListener("change", (ev) => {
      if (ev.target.checked) state.picked.add(d.domain);
      else state.picked.delete(d.domain);
      renderToolbar(profile);
    });
    head.querySelector(".keep").addEventListener("click", () => {
      toggleKeep(profile, d.domain);
      state.picked.delete(d.domain);
      renderProfile(profile);
    });
    head.querySelector(".danger").addEventListener("click", () =>
      deleteCookies(profile, d)
    );
    row.appendChild(head);

    const body = document.createElement("div");
    body.className = "site-body";
    const sessionSet = new Set(d.signals);
    const items = d.cookies
      .slice()
      .sort((a, b) => (sessionSet.has(b.name) ? 1 : 0) - (sessionSet.has(a.name) ? 1 : 0))
      .map((c) => {
        const isSession = sessionSet.has(c.name);
        const flags = [];
        if (c.secure) flags.push("Secure");
        if (c.http_only) flags.push("HttpOnly");
        const meta = [expiryText(c.expires_unix), ...flags].join(" · ");
        return `<div class="cookie-item">
          <span class="chip ${isSession ? "key" : ""}">${escapeHtml(c.name)}</span>
          <span class="cookie-meta">${escapeHtml(meta)}</span>
        </div>`;
      })
      .join("");
    body.innerHTML =
      `<div class="cookie-group-label">Cookies (${d.cookies.length})</div>` +
      `<div class="cookie-list">${items}</div>`;
    row.appendChild(body);
    frag.appendChild(row);
  }
  const container = document.createElement("div");
  // Cap tall lists so the section stays compact and scrolls internally
  // (keeps the panel headers and other sections reachable).
  container.className = "rows" + (domains.length > 8 ? " capped" : "");
  container.appendChild(frag);
  return container;
}

function loginRows(logins, profile) {
  const frag = document.createDocumentFragment();
  for (const l of logins) {
    const label = l.origin || "(unknown)";
    const row = document.createElement("div");
    row.className = "login-row";
    const user =
      l.username && l.username !== "(encrypted)"
        ? `<span class="user">${escapeHtml(l.username)}</span>`
        : "";
    row.innerHTML = `
      <span class="dot amber"></span>
      <span class="site-domain">${escapeHtml(label)}</span>
      ${user}
      <button class="icon-btn danger" type="button" title="Remove this saved password">${ICONS.trash}</button>
    `;
    row.querySelector(".icon-btn").addEventListener("click", () =>
      deletePassword(profile, l)
    );
    frag.appendChild(row);
  }
  const container = document.createElement("div");
  container.className = "rows" + (logins.length > 8 ? " capped" : "");
  container.appendChild(frag);
  return container;
}

// ---- toolbar (bulk actions) -------------------------------------------------

function allCookieDomains(p) {
  return [...p.logged_in_sites, ...p.other_cookie_domains].map((c) => c.domain);
}
function trackerDomains(p) {
  return [...p.logged_in_sites, ...p.other_cookie_domains]
    .filter((c) => c.is_tracker)
    .map((c) => c.domain);
}

function renderToolbar(p) {
  const bar = el("toolbar");
  bar.innerHTML = "";

  const keep = getKeep(p);
  const picked = [...state.picked].filter((dom) => !keep.has(dom));
  const trackers = trackerDomains(p).filter((dom) => !keep.has(dom));
  const allDomains = allCookieDomains(p).filter((dom) => !keep.has(dom));

  const mk = (label, cls, disabled, onClick, tip) => {
    const b = document.createElement("button");
    b.className = `tool-btn ${cls}` + (tip ? " tip" : "");
    b.type = "button";
    b.textContent = label;
    b.disabled = disabled;
    if (tip) b.dataset.tip = tip;
    if (!disabled) b.addEventListener("click", onClick);
    return b;
  };

  // Fixed action buttons — always first, so their position never shifts when a
  // selection appears. Each explains itself (and how it differs) on hover.
  bar.appendChild(
    mk(
      `Clear cache (${humanSize(p.cache.size_bytes)})`,
      "ghost",
      !p.cache.path || p.cache.size_bytes === 0,
      () => clearCache(p),
      "Empties this profile's HTTP cache — temporary files like images, scripts and styles. Frees disk space and makes sites re-download assets. Does NOT sign you out or remove any cookies."
    )
  );
  bar.appendChild(
    mk(
      `Remove trackers (${trackers.length})`,
      "ghost",
      trackers.length === 0,
      () => bulkRemove(p, trackers, `${trackers.length} tracker domain(s)`),
      "Deletes cookies only for known third-party ad/analytics domains (doubleclick, google-analytics, …). Your logins and normal site cookies are left untouched."
    )
  );
  bar.appendChild(
    mk(
      "Remove all cookies",
      "ghost",
      allDomains.length === 0,
      () => bulkRemove(p, allDomains, `all cookies in this profile`),
      "Deletes cookies for EVERY site in this profile, except ones you've kept with the star. Signs you out everywhere. Does not touch the cache or your saved passwords."
    )
  );

  // Selection actions appear to the RIGHT of the fixed buttons when active.
  if (picked.length) {
    const divider = document.createElement("span");
    divider.className = "tool-divider";
    bar.appendChild(divider);
    bar.appendChild(
      mk(`Remove selected (${picked.length})`, "danger", false, () =>
        bulkRemove(p, picked, `${picked.length} selected site(s)`)
      )
    );
    bar.appendChild(
      mk("Clear selection", "ghost", false, () => {
        state.picked.clear();
        renderProfile(p);
      })
    );
  }
}

async function bulkRemove(profile, domains, label) {
  if (!domains.length) return;
  const kept = getKeep(profile);
  const targets = domains.filter((dom) => !kept.has(dom));
  if (!targets.length) {
    toast("Everything matched is on the keep-list — nothing removed.");
    return;
  }
  const ok = await confirmModal({
    title: "Remove cookies?",
    body: `This permanently deletes cookies for ${label} from ${profile.browser} (${targets.length} domain(s)). Kept sites are skipped. You'll be signed out of those sites. This can't be undone.`,
    warn: profile.browser_running
      ? `${profile.browser} is running — quit it first, or it may restore these on exit.`
      : null,
  });
  if (!ok) return;
  try {
    const n = await invoke("delete_cookies_bulk", {
      profilePath: profile.path,
      family: profile.family,
      domains: targets,
    });
    await runScan();
    toast(`Removed ${n} cookie${n === 1 ? "" : "s"} across ${targets.length} site(s).`);
  } catch (e) {
    toast(`Bulk removal failed: ${e && e.message ? e.message : e}`, true);
  }
}

async function clearCache(profile) {
  const ok = await confirmModal({
    title: "Clear HTTP cache?",
    body: `This empties ${profile.browser}'s cache for this profile (${humanSize(profile.cache.size_bytes)}). It only frees disk space and makes sites reload assets — it does not log you out. This can't be undone.`,
    warn: profile.browser_running
      ? `${profile.browser} is running — some cache files may be locked and skipped until you quit it.`
      : null,
  });
  if (!ok) return;
  try {
    const freed = await invoke("clear_cache", { cachePath: profile.cache.path });
    await runScan();
    toast(`Cleared cache — freed ${humanSize(freed)}.`);
  } catch (e) {
    toast(`Couldn't clear cache: ${e && e.message ? e.message : e}`, true);
  }
}

// ---- deletion + modal + toast ----------------------------------------------

async function deleteCookies(profile, d) {
  const ok = await confirmModal({
    title: `Remove cookies for ${d.domain}?`,
    body: `This permanently deletes all ${d.cookie_count} cookie(s) for ${d.domain} from ${profile.browser} — you'll be signed out of that site. This can't be undone.`,
    warn: profile.browser_running
      ? `${profile.browser} is running — quit it first, or it may restore these cookies on exit.`
      : null,
  });
  if (!ok) return;
  try {
    const n = await invoke("delete_cookies", {
      profilePath: profile.path,
      family: profile.family,
      domain: d.domain,
    });
    await runScan();
    toast(`Removed ${n} cookie${n === 1 ? "" : "s"} for ${d.domain}.`);
  } catch (e) {
    toast(`Couldn't remove cookies: ${e && e.message ? e.message : e}`, true);
  }
}

async function deletePassword(profile, l) {
  const label = l.origin || "(unknown)";
  const ok = await confirmModal({
    title: "Remove saved password?",
    body: `This permanently deletes the saved password for ${label} from ${profile.browser}. This can't be undone.`,
    warn: profile.browser_running
      ? `${profile.browser} is running — quit it first, or it may restore this entry on exit.`
      : null,
  });
  if (!ok) return;
  try {
    const n = await invoke("delete_password", {
      profilePath: profile.path,
      family: profile.family,
      origin: l.origin || "",
      username: l.username || "",
    });
    await runScan();
    toast(n > 0 ? `Removed saved password for ${label}.` : "Nothing matched to remove.");
  } catch (e) {
    toast(`Couldn't remove password: ${e && e.message ? e.message : e}`, true);
  }
}

let modalResolver = null;
function confirmModal({ title, body, warn }) {
  el("modalTitle").textContent = title;
  el("modalBody").textContent = body;
  const warnEl = el("modalWarn");
  if (warn) {
    warnEl.textContent = warn;
    warnEl.hidden = false;
  } else {
    warnEl.hidden = true;
  }
  el("modal").hidden = false;
  return new Promise((resolve) => {
    modalResolver = resolve;
  });
}

function closeModal(result) {
  el("modal").hidden = true;
  if (modalResolver) {
    modalResolver(result);
    modalResolver = null;
  }
}

let toastTimer = null;
function toast(msg, isError = false) {
  const t = el("toast");
  t.textContent = msg;
  t.className = "toast" + (isError ? " error" : "");
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => {
    t.hidden = true;
  }, 3400);
}

function cacheSection(cache) {
  const sec = document.createElement("section");
  sec.className = "panel";
  sec.innerHTML = `
    <div class="panel-head static">
      <span class="panel-title">HTTP cache</span>
      <span class="badge cyan">${humanSize(cache.size_bytes)}</span>
    </div>
    <div class="panel-body">
      <div class="cache-path">${
        cache.path ? escapeHtml(cache.path) : "Cache directory not found."
      }</div>
    </div>
  `;
  return sec;
}

// ---- wiring -----------------------------------------------------------------

scanBtn.addEventListener("click", runScan);
window.addEventListener("DOMContentLoaded", runScan);
el("filter").addEventListener("input", (e) => {
  state.filter = e.target.value.trim().toLowerCase();
  if (state.profiles.length) renderProfile(state.profiles[state.selected]);
});

el("modalConfirm").addEventListener("click", () => closeModal(true));
el("modalCancel").addEventListener("click", () => closeModal(false));
el("modal").addEventListener("click", (e) => {
  if (e.target.id === "modal") closeModal(false); // click backdrop = cancel
});
document.addEventListener("keydown", (e) => {
  if (!el("modal").hidden && e.key === "Escape") closeModal(false);
});
