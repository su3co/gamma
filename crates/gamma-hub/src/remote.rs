//! Remote simulation hosts over SSH.

use anyhow::{Context, Result, bail};
use gamma_core::project::RemoteCfg;
use serde_json::{Value, json};
use std::path::Path;
use tokio::process::Command;

pub const DEFAULT_PROGRAM: &str = "~/.local/share/gamma/bin/gamma";
pub const DEFAULT_WORKDIR: &str = "~/.cache/gamma/jobs";

pub fn program(r: &RemoteCfg) -> String {
    r.program.clone().unwrap_or_else(|| DEFAULT_PROGRAM.into())
}

pub fn workdir(r: &RemoteCfg) -> String {
    r.workdir.clone().unwrap_or_else(|| DEFAULT_WORKDIR.into())
}

/// `ssh` with batch mode (no password prompts; keys or an agent are required).
pub fn ssh(r: &RemoteCfg) -> Command {
    let mut c = Command::new("ssh");
    c.args([
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=15",
        "-o",
        "ServerAliveInterval=30",
    ])
    .arg(&r.ssh);
    c.kill_on_drop(true);
    c
}

/// Quote for a POSIX shell, leaving a leading `~/` to the remote shell.
pub fn sh_quote(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/") {
        return format!("~/{}", sh_quote(rest));
    }
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-:=,+@".contains(c))
    {
        return s.to_string();
    }
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub async fn run_remote(r: &RemoteCfg, script: &str) -> Result<String> {
    let out = ssh(r).arg(script).output().await.context("running ssh")?;
    if !out.status.success() {
        bail!(
            "ssh {}: {}",
            r.ssh,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Copy a local file to the host through ssh's standard input.
pub async fn put(r: &RemoteCfg, local: &Path, remote: &str) -> Result<()> {
    use tokio::io::AsyncWriteExt;
    let data = tokio::fs::read(local).await?;
    let dir = remote.rsplit_once('/').map(|(d, _)| d).unwrap_or(".");
    let mut child = ssh(r)
        .arg(format!(
            "mkdir -p {} && cat > {}",
            sh_quote(dir),
            sh_quote(remote)
        ))
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    child.stdin.take().unwrap().write_all(&data).await?;
    let out = child.wait_with_output().await?;
    if !out.status.success() {
        bail!(
            "copying to {}: {}",
            r.ssh,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

/// Copy a file from the host.
pub async fn get(r: &RemoteCfg, remote: &str, local: &Path) -> Result<()> {
    let out = ssh(r)
        .arg(format!("cat {}", sh_quote(remote)))
        .output()
        .await?;
    if !out.status.success() {
        bail!(
            "copying from {}: {}",
            r.ssh,
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    if let Some(d) = local.parent() {
        tokio::fs::create_dir_all(d).await?;
    }
    gamma_core::fsutil::atomic_write(local, &out.stdout)?;
    Ok(())
}

/// What the host has: architecture, the gamma program and its devices.
pub async fn test(r: &RemoteCfg) -> Result<Value> {
    let prog = program(r);
    let script = format!(
        "uname -sm; {} worker info 2>&1 || echo GAMMA_MISSING",
        sh_quote(&prog)
    );
    let out = run_remote(r, &script).await?;
    let mut lines = out.lines();
    let uname = lines.next().unwrap_or("").to_string();
    let rest: Vec<&str> = lines.collect();
    if rest.iter().any(|l| l.contains("GAMMA_MISSING")) {
        return Ok(
            json!({"reachable": true, "uname": uname, "gamma": null, "message": format!("gamma is not installed at {prog} on {}; run `gamma remote install {}`", r.ssh, r.name)}),
        );
    }
    let hello = rest.iter().find_map(|l| gamma_sim::stream::Msg::parse(l));
    Ok(json!({"reachable": true, "uname": uname, "gamma": hello}))
}

/// Install a gamma program (a Linux build for the host's architecture) on the host.
pub async fn install(r: &RemoteCfg, binary: &Path) -> Result<Value> {
    let prog = program(r);
    let tmp = format!("{prog}.new");
    put(r, binary, &tmp).await?;
    run_remote(
        r,
        &format!(
            "chmod +x {t} && mv -f {t} {p} && {p} --version",
            t = sh_quote(&tmp),
            p = sh_quote(&prog)
        ),
    )
    .await?;
    test(r).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoting() {
        assert_eq!(sh_quote("~/.cache/gamma"), "~/.cache/gamma");
        assert_eq!(sh_quote("a b"), "'a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
    }
}
