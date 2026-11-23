// gamma's desktop shell: starts (or finds) the hub, opens the window, and
// runs updates with channels, signature checks and rollback.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod update;

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::Mutex;

struct Shell {
    hub: Mutex<Option<gamma_hub::HubInfo>>,
}

/// The `gamma` program bundled next to this executable (Tauri's externalBin),
/// or on PATH in development.
fn gamma_program() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let name = if cfg!(windows) { "gamma.exe" } else { "gamma" };
    if let Some(dir) = exe.parent() {
        let p = dir.join(name);
        if p.is_file() {
            return p;
        }
    }
    if let Ok(p) = std::env::var("GAMMA_PROGRAM") {
        return PathBuf::from(p);
    }
    PathBuf::from(name)
}

#[tauri::command]
fn hub_address(state: tauri::State<Shell>) -> Result<Value, String> {
    let mut g = state.hub.lock().unwrap();
    if let Some(h) = g.as_ref()
        && gamma_hub::discovery::alive(h)
    {
        return Ok(json!({"url": h.url(), "token": h.token}));
    }
    let h =
        gamma_hub::discovery::connect_or_spawn_with(&gamma_program(), &["--idle-exit-min", "30"])
            .map_err(|e| format!("{e:#}"))?;
    let v = json!({"url": h.url(), "token": h.token});
    *g = Some(h);
    Ok(v)
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> Value {
    json!({
        "version": app.package_info().version.to_string(),
        "install_kind": update::install_kind().as_str(),
        "rollback_version": update::rollback_version(),
        "gamma": gamma_program(),
    })
}

#[tauri::command]
fn mark_healthy() -> Result<(), String> {
    update::mark_healthy().map_err(|e| format!("{e:#}"))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let headless = update::headless_command(&args);
    // an update that failed to start twice is rolled back before anything else
    update::on_start(headless.is_some());
    let windowed = headless.is_none();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(Shell {
            hub: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            hub_address,
            app_info,
            mark_healthy,
            update::update_check,
            update::update_install,
            update::update_rollback
        ])
        .setup(move |app| {
            if windowed {
                let cfg = app
                    .config()
                    .app
                    .windows
                    .first()
                    .cloned()
                    .expect("window config");
                tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?.build()?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while starting gamma");
    if let Some(cmd) = headless {
        // scriptable update commands (CI and tests): no window
        let code = tauri::async_runtime::block_on(update::run_headless(app.handle().clone(), cmd));
        std::process::exit(code);
    }
    app.run(|_, _| {});
}
