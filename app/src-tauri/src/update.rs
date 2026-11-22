//! Updates: signed feeds per channel (Tauri updater, ed25519 signatures
//! checked against the public key compiled into the app), a kept copy of the
//! previous version, probation for a newly installed version, and rollback.
//!
//! A new version starts on probation. The window reports a healthy start
//! (hub reachable, interface loaded) a few seconds after launch; if two
//! launches in a row do not, the third launch restores the kept version
//! before it opens anything. A person can also go back from Settings.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tauri_plugin_updater::UpdaterExt;

/// Base URL of the update feeds; `<base>/<channel>/latest.json`. Set at build
/// time (`GAMMA_UPDATE_BASE`) or at run time (same variable) for test feeds.
pub fn feed_url(channel: &str) -> String {
    let base = std::env::var("GAMMA_UPDATE_BASE")
        .ok()
        .or_else(|| option_env!("GAMMA_UPDATE_BASE").map(|s| s.to_string()))
        .unwrap_or_else(|| "https://updates.invalid/gamma".into());
    let ch = if channel == "beta" { "beta" } else { "stable" };
    format!("{}/{ch}/latest.json", base.trim_end_matches('/'))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InstallKind {
    MacApp,
    AppImage,
    Package,
    Windows,
    Dev,
}

impl InstallKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            InstallKind::MacApp => "app",
            InstallKind::AppImage => "appimage",
            InstallKind::Package => "package",
            InstallKind::Windows => "windows",
            InstallKind::Dev => "dev",
        }
    }
}

pub fn install_kind() -> InstallKind {
    let exe = std::env::current_exe().unwrap_or_default();
    if cfg!(target_os = "macos") {
        if bundle_path().is_some() {
            InstallKind::MacApp
        } else {
            InstallKind::Dev
        }
    } else if cfg!(target_os = "linux") {
        if std::env::var_os("APPIMAGE").is_some() {
            InstallKind::AppImage
        } else if exe.starts_with("/usr") || exe.starts_with("/opt") {
            InstallKind::Package
        } else {
            InstallKind::Dev
        }
    } else if cfg!(windows) {
        InstallKind::Windows
    } else {
        InstallKind::Dev
    }
}

/// The `.app` folder this executable runs from (macOS).
fn bundle_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    (app.extension().map(|e| e == "app").unwrap_or(false)).then(|| app.to_path_buf())
}

/// What is replaced by an update: the `.app` folder or the AppImage file.
fn installed_path() -> Option<PathBuf> {
    match install_kind() {
        InstallKind::MacApp => bundle_path(),
        InstallKind::AppImage => std::env::var_os("APPIMAGE").map(PathBuf::from),
        _ => None,
    }
}

fn state_dir() -> PathBuf {
    std::env::var_os("GAMMA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("gamma")
        })
        .join("updates")
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct UpdateState {
    /// A newly installed version that has not yet reported a healthy start.
    pub probation: bool,
    pub failed_starts: u32,
    pub previous_version: Option<String>,
    pub installed_version: Option<String>,
    pub backup: Option<PathBuf>,
}

fn state_path() -> PathBuf {
    state_dir().join("state.json")
}

pub fn load_state() -> UpdateState {
    std::fs::read_to_string(state_path())
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn save_state(s: &UpdateState) -> Result<()> {
    std::fs::create_dir_all(state_dir())?;
    let p = state_path();
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(s)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

pub fn rollback_version() -> Option<String> {
    let s = load_state();
    s.backup
        .as_ref()
        .filter(|b| b.exists())
        .and(s.previous_version)
}

/// Copy a folder or file, keeping symlinks and signatures (ditto on macOS).
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if to.exists() {
        if to.is_dir() {
            std::fs::remove_dir_all(to)?
        } else {
            std::fs::remove_file(to)?
        }
    }
    if let Some(p) = to.parent() {
        std::fs::create_dir_all(p)?;
    }
    if from.is_dir() {
        #[cfg(target_os = "macos")]
        {
            let st = std::process::Command::new("ditto")
                .arg(from)
                .arg(to)
                .status()?;
            anyhow::ensure!(st.success(), "ditto failed");
            return Ok(());
        }
        #[cfg(not(target_os = "macos"))]
        {
            let st = std::process::Command::new("cp")
                .arg("-a")
                .arg(from)
                .arg(to)
                .status()?;
            anyhow::ensure!(st.success(), "cp failed");
            return Ok(());
        }
    }
    std::fs::copy(from, to)?;
    Ok(())
}

/// Keep the running version before an update replaces it.
fn backup_current(version: &str) -> Result<PathBuf> {
    let src = installed_path().context("this installation does not update itself")?;
    let name = src.file_name().context("no file name")?.to_owned();
    let dst = state_dir().join("previous").join(name);
    copy_tree(&src, &dst)?;
    let mut s = load_state();
    s.previous_version = Some(version.to_string());
    s.backup = Some(dst.clone());
    save_state(&s)?;
    Ok(dst)
}

/// Put the kept version back in place.
fn restore() -> Result<String> {
    let s = load_state();
    let backup = s
        .backup
        .clone()
        .filter(|b| b.exists())
        .context("no earlier version is kept")?;
    let target = installed_path().context("this installation does not update itself")?;
    let aside = target.with_extension("replaced");
    let _ = std::fs::remove_dir_all(&aside);
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(&target, &aside).context("moving the current version aside")?;
    if let Err(e) = copy_tree(&backup, &target) {
        let _ = std::fs::rename(&aside, &target);
        return Err(e).context("restoring the earlier version");
    }
    let _ = if aside.is_dir() {
        std::fs::remove_dir_all(&aside)
    } else {
        std::fs::remove_file(&aside)
    };
    let v = s.previous_version.clone().unwrap_or_default();
    save_state(&UpdateState {
        probation: false,
        failed_starts: 0,
        previous_version: None,
        installed_version: Some(v.clone()),
        backup: None,
    })?;
    Ok(v)
}

fn relaunch() -> ! {
    if let Some(p) = installed_path() {
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg("-n").arg(&p).spawn();
        #[cfg(not(target_os = "macos"))]
        let _ = std::process::Command::new(&p).spawn();
    }
    std::process::exit(0)
}

/// Called first thing at start-up.
pub fn on_start() {
    let mut s = load_state();
    if !s.probation {
        return;
    }
    s.failed_starts += 1;
    if s.failed_starts >= 3 {
        eprintln!(
            "gamma: this version did not start properly twice; going back to {}",
            s.previous_version.clone().unwrap_or_default()
        );
        if restore().is_ok() {
            relaunch();
        }
        s.probation = false;
    }
    let _ = save_state(&s);
}

pub fn mark_healthy() -> Result<()> {
    let mut s = load_state();
    if s.probation || s.failed_starts > 0 {
        s.probation = false;
        s.failed_starts = 0;
        save_state(&s)?;
    }
    Ok(())
}

async fn updater(app: &tauri::AppHandle, channel: &str) -> Result<tauri_plugin_updater::Updater> {
    // https only, except in test builds made for a feed on this machine
    // (scripts/package.sh sets dangerousInsecureTransportProtocol for those)
    let url: tauri::Url = feed_url(channel).parse()?;
    Ok(app.updater_builder().endpoints(vec![url])?.build()?)
}

#[tauri::command]
pub async fn update_check(app: tauri::AppHandle, channel: String) -> Result<Value, String> {
    if matches!(install_kind(), InstallKind::Package) {
        return Ok(json!({"available": false, "managed_by": "package manager"}));
    }
    let up = updater(&app, &channel)
        .await
        .map_err(|e| format!("{e:#}"))?;
    match up.check().await.map_err(|e| e.to_string())? {
        Some(u) => Ok(
            json!({"available": true, "version": u.version, "notes": u.body, "date": u.date.map(|d| d.to_string()), "feed": feed_url(&channel)}),
        ),
        None => Ok(json!({"available": false, "feed": feed_url(&channel)})),
    }
}

#[tauri::command]
pub async fn update_install(app: tauri::AppHandle, channel: String) -> Result<(), String> {
    let up = updater(&app, &channel)
        .await
        .map_err(|e| format!("{e:#}"))?;
    let u = up
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or("no update available")?;
    let current = app.package_info().version.to_string();
    backup_current(&current)
        .map_err(|e| format!("keeping the current version failed, update not installed: {e:#}"))?;
    // download, verify the ed25519 signature, replace the installation
    u.download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("update failed, nothing was replaced: {e}"))?;
    let mut s = load_state();
    s.probation = true;
    s.failed_starts = 0;
    s.installed_version = Some(u.version.clone());
    save_state(&s).map_err(|e| e.to_string())?;
    app.restart();
}

#[tauri::command]
pub async fn update_rollback() -> Result<(), String> {
    restore().map_err(|e| format!("{e:#}"))?;
    relaunch();
}

/// Commands that run without a window:
/// `--version-info`, `--check-update [channel]`, `--self-update [channel]`, `--rollback`.
#[derive(Clone, Debug)]
pub enum Headless {
    Info,
    Check(String),
    Update(String),
    Rollback,
}

pub fn headless_command(args: &[String]) -> Option<Headless> {
    let ch = |i: usize| {
        args.get(i)
            .filter(|a| !a.starts_with("--"))
            .cloned()
            .unwrap_or_else(|| "stable".into())
    };
    for (i, a) in args.iter().enumerate() {
        match a.as_str() {
            "--version-info" => return Some(Headless::Info),
            "--check-update" => return Some(Headless::Check(ch(i + 1))),
            "--self-update" => return Some(Headless::Update(ch(i + 1))),
            "--rollback" => return Some(Headless::Rollback),
            _ => {}
        }
    }
    None
}

fn info_json(app: &tauri::AppHandle) -> Value {
    let s = load_state();
    json!({"version": app.package_info().version.to_string(), "install_kind": install_kind().as_str(), "probation": s.probation, "failed_starts": s.failed_starts, "rollback_version": rollback_version()})
}

pub async fn run_headless(app: tauri::AppHandle, cmd: Headless) -> i32 {
    let r: Result<Value, String> = match cmd {
        Headless::Info => Ok(info_json(&app)),
        Headless::Check(ch) => update_check(app.clone(), ch).await,
        Headless::Update(ch) => {
            async {
                let up = updater(&app, &ch).await.map_err(|e| format!("{e:#}"))?;
                let Some(u) = up.check().await.map_err(|e| e.to_string())? else {
                    return Ok(json!({"updated": false, "reason": "no update available"}));
                };
                let current = app.package_info().version.to_string();
                backup_current(&current).map_err(|e| {
                    format!("keeping the current version failed, update not installed: {e:#}")
                })?;
                u.download_and_install(|_, _| {}, || {})
                    .await
                    .map_err(|e| format!("update failed, nothing was replaced: {e}"))?;
                let mut s = load_state();
                s.probation = true;
                s.failed_starts = 0;
                s.installed_version = Some(u.version.clone());
                save_state(&s).map_err(|e| e.to_string())?;
                Ok(json!({"updated": true, "from": current, "to": u.version}))
            }
            .await
        }
        Headless::Rollback => restore()
            .map(|v| json!({"restored": v}))
            .map_err(|e| format!("{e:#}")),
    };
    match r {
        Ok(v) => {
            println!("{v}");
            0
        }
        Err(e) => {
            println!("{}", json!({"error": e}));
            1
        }
    }
}
