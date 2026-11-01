//! Tenseiga on Android: the same client (app.rs), started by the NativeActivity.
//!
//! Everything lives in the app's own folder on shared storage,
//! Android/data/io.github.mythraxzs.tenseiga/files/ (copy the game data there over USB):
//!   config/local.toml   the config, written on first start (set the server there)
//!   ro/                 game data: data.grf, BGM/, System/ and kro/ (as in the Steam Deck package)
//!   english/            the English translation (optional)
//!   tenseiga.log        the log
//! The Samurai overlay ships inside the APK and is unpacked to overlays/ on every start.
#![cfg(target_os = "android")]
#![allow(dead_code)]

include!("app.rs");

use winit::platform::android::activity::AndroidApp;
use winit::platform::android::EventLoopBuilderExtAndroid;

#[no_mangle]
fn android_main(android: AndroidApp) {
    let home = android.external_data_path().or_else(|| android.internal_data_path()).unwrap_or_else(|| PathBuf::from("."));
    let _ = std::fs::create_dir_all(home.join("config"));
    let _ = std::env::set_current_dir(&home);
    // No console here: the log goes to a file beside the config.
    let log: Box<dyn std::io::Write + Send> = match std::fs::File::create(home.join("tenseiga.log")) {
        Ok(f) => Box::new(f),
        Err(_) => Box::new(std::io::sink()),
    };
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(log))
        .with_env_filter(tracing_subscriber::EnvFilter::new("info"))
        .init();
    std::panic::set_hook(Box::new(|info| tracing::error!("panic: {info}")));
    unpack_overlays(&android, &home);
    first_start(&home);
    std::env::set_var("ROCLIENT_CONFIG", home.join("config").join("local.toml"));
    // Game controllers are read from the input events before winit sees them (gamepad.rs).
    winit::platform::android::activity::set_input_filter(gamepad::android::input);
    let event_loop = match EventLoop::builder().with_android_app(android).build() {
        Ok(e) => e,
        Err(e) => {
            tracing::error!("event loop: {e}");
            return;
        }
    };
    if let Err(e) = app_start::start(Some(event_loop)) {
        tracing::error!("Tenseiga could not start: {e:#}");
    }
}

/// The APK's assets/overlays (listed in overlays/index.txt) into the app folder, where
/// samurai::overlay_path finds them.
fn unpack_overlays(android: &AndroidApp, home: &std::path::Path) {
    use std::io::Read;
    let assets = android.asset_manager();
    let read = |path: &str| -> Option<Vec<u8>> {
        let name = std::ffi::CString::new(path).ok()?;
        let mut asset = assets.open(&name)?;
        let mut data = vec![];
        asset.read_to_end(&mut data).ok()?;
        Some(data)
    };
    let Some(index) = read("overlays/index.txt") else {
        tracing::warn!("no overlays in the APK");
        return;
    };
    let mut n = 0;
    for path in String::from_utf8_lossy(&index).lines().map(str::trim).filter(|p| !p.is_empty()) {
        let Some(data) = read(path) else { continue };
        let to = home.join(path);
        if let Some(dir) = to.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::read(&to).ok().as_deref() != Some(&data[..]) && std::fs::write(&to, &data).is_ok() {
            n += 1;
        }
    }
    tracing::info!(updated = n, "overlays unpacked");
}

/// A first config pointing at the app folder's data, and a UI scale for a phone's dense screen.
fn first_start(home: &std::path::Path) {
    let config = home.join("config").join("local.toml");
    if !config.exists() {
        let dir = home.display();
        let text = format!(
            "# Tenseiga on Android. Set login_addr to your rAthena server.\n\
             login_addr = \"127.0.0.1:6900\"\n\
             username = \"\"\n\
             password = \"\"\n\
             char_slot = 0\n\
             backend = \"vulkan\"\n\
             # Game data copied into the app folder (as in the Steam Deck package).\n\
             client_dir = \"{dir}/ro\"\n\
             # iRO first; kRO only fills what iRO lacks.\n\
             grfs = [\"data.grf\", \"kro/data.grf\", \"kro/rdata.grf\"]\n\
             # English translation (github.com/llchrisll/ROenglishRE).\n\
             overlays = [\"{dir}/english\"]\n\
             map = \"prontera\"\n\
             msaa = 4\n"
        );
        if let Err(e) = std::fs::write(&config, text) {
            tracing::error!("write {}: {e}", config.display());
        }
    }
    let settings = home.join("config").join("settings.toml");
    if !settings.exists() {
        let _ = std::fs::write(settings, "ui_scale = 0.6\n");
    }
}
