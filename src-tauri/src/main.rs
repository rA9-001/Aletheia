// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // WebKitGTK's default DMABUF/GPU rendering path is broken on many Wayland
    // setups (notably Arch/CachyOS + KDE), producing
    // "Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display"
    // and a webview that never repaints — the UI appears stuck on its loading
    // state. Forcing the software/compositing-safe path fixes it. These are set
    // before GTK initializes and only if the user hasn't overridden them.
    #[cfg(target_os = "linux")]
    {
        for (k, v) in [
            ("WEBKIT_DISABLE_DMABUF_RENDERER", "1"),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
        ] {
            if std::env::var_os(k).is_none() {
                // Safe: this runs at the very start of `main`, before any
                // threads are spawned, so there is no concurrent env access.
                // (Also silences the Rust 2024 `deprecated_safe_2024` lint,
                // where `set_var` becomes an unsafe function.)
                unsafe {
                    std::env::set_var(k, v);
                }
            }
        }
    }

    aletheia_lib::run()
}
