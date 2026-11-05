//! Tenseiga
//!
//!   ro-client --headless   run login -> char select -> map entry and exit
//!   ro-client              open the renderer window

// Release builds are a windowed app: no console window when started by double-click.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// The client itself, shared with the Android library (lib.rs).
include!("app.rs");

fn main() {
    find_home();
    if let Err(e) = app_start::run() {
        fatal(&format!("{e:#}"));
    }
}

/// Started by double-click (or from anywhere else), the working folder is wherever the system
/// chose. Unless ROCLIENT_CONFIG names the config, work from the folder that holds
/// `config\local.toml`: this one, else beside the exe or above it (the repo root, for a
/// build in target\release).
fn find_home() {
    if std::env::var_os("ROCLIENT_CONFIG").is_some() || std::path::Path::new("config/local.toml").is_file() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    if let Some(dir) = exe.ancestors().skip(1).find(|d| d.join("config").join("local.toml").is_file()) {
        let _ = std::env::set_current_dir(dir);
    }
}

/// A start-up failure: said in the log (console and tenseiga.log), and in a message box,
/// since a windowed app started by double-click has no console to say it on.
fn fatal(msg: &str) {
    tracing::error!("Tenseiga could not start: {msg}");
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
        let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
        let (text, title) = (wide(&format!("Tenseiga could not start.

{msg}")), wide("Tenseiga"));
        // SAFETY: both strings are NUL-terminated UTF-16 that outlive the call.
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | MB_ICONERROR) };
    }
    std::process::exit(1);
}
