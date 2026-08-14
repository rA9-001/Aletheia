mod scan;

use scan::ScanResult;

/// Scan the current user's own browser profiles and return the audit result.
#[tauri::command]
fn scan_profiles() -> ScanResult {
    scan::scan_all()
}

/// Delete all cookies for a domain in a specific profile. Returns rows removed.
#[tauri::command]
fn delete_cookies(profile_path: String, family: String, domain: String) -> Result<usize, String> {
    scan::delete_cookies_for_domain(&profile_path, &family, &domain)
}

/// Delete a saved password entry. Returns entries removed.
#[tauri::command]
fn delete_password(
    profile_path: String,
    family: String,
    origin: String,
    username: String,
) -> Result<usize, String> {
    scan::delete_saved_password(&profile_path, &family, &origin, &username)
}

/// Delete all cookies for the given set of domains. Returns rows removed.
#[tauri::command]
fn delete_cookies_bulk(
    profile_path: String,
    family: String,
    domains: Vec<String>,
) -> Result<usize, String> {
    scan::delete_cookies_bulk(&profile_path, &family, &domains)
}

/// Empty a profile's HTTP cache. Returns bytes freed.
#[tauri::command]
fn clear_cache(cache_path: String) -> Result<u64, String> {
    scan::clear_cache(&cache_path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Set the window/taskbar icon at runtime. On Linux this is what
            // makes the icon appear in the taskbar/dock (the generic "W" is the
            // WM fallback when no window icon is set).
            use tauri::Manager;
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(icon) =
                    tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png"))
                {
                    let _ = win.set_icon(icon);
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_profiles,
            delete_cookies,
            delete_password,
            delete_cookies_bulk,
            clear_cache
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aletheia");
}
