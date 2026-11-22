//! Finding the hub: `<runtime dir>/gamma/hub.json` names its port, process
//! and access token; clients start a hub when none answers.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HubInfo {
    pub port: u16,
    pub pid: u32,
    pub token: String,
    pub version: String,
}

impl HubInfo {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

pub fn runtime_dir() -> PathBuf {
    if let Ok(d) = std::env::var("GAMMA_RUNTIME_DIR") {
        return PathBuf::from(d);
    }
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("gamma")
}

pub fn hub_file() -> PathBuf {
    runtime_dir().join("hub.json")
}

pub fn read_hub_info() -> Option<HubInfo> {
    let t = std::fs::read_to_string(hub_file()).ok()?;
    serde_json::from_str(&t).ok()
}

pub fn write_hub_info(info: &HubInfo) -> Result<()> {
    let p = hub_file();
    gamma_core::fsutil::atomic_write(&p, serde_json::to_string_pretty(info)?.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600))?;
        std::fs::set_permissions(runtime_dir(), std::fs::Permissions::from_mode(0o700)).ok();
    }
    Ok(())
}

pub fn remove_hub_info(pid: u32) {
    if read_hub_info().map(|h| h.pid == pid).unwrap_or(false) {
        let _ = std::fs::remove_file(hub_file());
    }
}

/// Is a hub answering at `info`? (A plain TCP probe plus a version request.)
pub fn alive(info: &HubInfo) -> bool {
    use std::io::{Read, Write};
    let Ok(mut s) = std::net::TcpStream::connect_timeout(
        &([127, 0, 0, 1], info.port).into(),
        Duration::from_millis(400),
    ) else {
        return false;
    };
    let _ = s.set_read_timeout(Some(Duration::from_millis(1500)));
    let req = format!(
        "GET /api/hub HTTP/1.0\r\nHost: 127.0.0.1\r\nAuthorization: Bearer {}\r\n\r\n",
        info.token
    );
    if s.write_all(req.as_bytes()).is_err() {
        return false;
    }
    let mut buf = [0u8; 64];
    match s.read(&mut buf) {
        Ok(n) => String::from_utf8_lossy(&buf[..n]).contains(" 200 "),
        Err(_) => false,
    }
}

/// The running hub, or a new one started from `program hub --detach`.
pub fn connect_or_spawn(program: &std::path::Path) -> Result<HubInfo> {
    connect_or_spawn_with(program, &[])
}

/// As [`connect_or_spawn`], passing extra arguments to a new hub (e.g. `--idle-exit-min 30`).
pub fn connect_or_spawn_with(program: &std::path::Path, args: &[&str]) -> Result<HubInfo> {
    if let Some(i) = read_hub_info() {
        if alive(&i) {
            return Ok(i);
        }
    }
    let mut cmd = std::process::Command::new(program);
    cmd.arg("hub")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.spawn()
        .with_context(|| format!("starting the hub ({})", program.display()))?;
    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(50));
        if let Some(i) = read_hub_info() {
            if alive(&i) {
                return Ok(i);
            }
        }
    }
    bail!("the hub did not start within 5 s; run `gamma hub` in a terminal to see why")
}
