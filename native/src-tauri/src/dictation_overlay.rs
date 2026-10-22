//! Presentation of the recording overlay window.
//!
//! The overlay is a separate, lazily created Tauri webview that shares the
//! main bundle. It is button-only: the editable review lives in the main
//! window, which stays focusable. The overlay is presented with the public
//! `orderFrontRegardless` path so it never becomes the key window and never
//! takes focus away from the app the user is dictating into. The Dock and main
//! window behavior are untouched.
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const OVERLAY_LABEL: &str = "overlay";
const OVERLAY_URL: &str = "index.html?overlay=1";

/// Present the overlay without activating it. The window is created on first
/// use, so a hidden overlay costs nothing until dictation starts.
pub fn present(app: &AppHandle, always_on_top: bool) -> Result<(), String> {
    let window = match app.get_webview_window(OVERLAY_LABEL) {
        Some(window) => window,
        None => WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App(OVERLAY_URL.into()))
            .title("Golos Dictation")
            .inner_size(380.0, 220.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(always_on_top)
            .skip_taskbar(true)
            .visible(false)
            .focused(false)
            .build()
            .map_err(|error| format!("The dictation overlay window is unavailable: {error}"))?,
    };
    let _ = window.set_always_on_top(always_on_top);
    // A non-focusable window cannot steal the key window from the target app.
    let _ = window.set_focusable(false);
    #[cfg(target_os = "macos")]
    {
        let pointer = window
            .ns_window()
            .map_err(|error| format!("The dictation overlay window is unavailable: {error}"))?;
        if pointer.is_null() {
            return Err("The dictation overlay window is unavailable.".into());
        }
        // Public NSWindow path: order the window in front without making it
        // key and without the NSPanel-only style bits.
        let ns_window = unsafe { &*(pointer as *const objc2_app_kit::NSWindow) };
        ns_window.orderFrontRegardless();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window.show();
    }
    Ok(())
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(OVERLAY_LABEL) {
        let _ = window.hide();
    }
}

/// Bring the main window forward for the explicit review step.
pub fn focus_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
