//! git through the `git` program: checkpoints are commits tagged
//! `gamma/checkpoint/<n>`.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").arg("-C").arg(root).args(args).output().context("running git (is it installed?)")?;
    if !out.status.success() {
        bail!("git {} failed: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

pub fn available() -> bool {
    Command::new("git").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

pub fn is_repo(root: &Path) -> bool {
    git(root, &["rev-parse", "--is-inside-work-tree"]).map(|s| s.trim() == "true").unwrap_or(false)
}

pub fn init(root: &Path) -> Result<()> {
    if !is_repo(root) {
        git(root, &["init", "-q", "-b", "main"])?;
    }
    Ok(())
}

fn identity_args(root: &Path) -> Vec<String> {
    let mut a = vec![];
    if git(root, &["config", "user.name"]).map(|s| s.trim().is_empty()).unwrap_or(true) {
        a.extend(["-c".to_string(), "user.name=gamma".to_string()]);
    }
    if git(root, &["config", "user.email"]).map(|s| s.trim().is_empty()).unwrap_or(true) {
        a.extend(["-c".to_string(), "user.email=gamma@localhost".to_string()]);
    }
    a
}

#[derive(Clone, Debug, Serialize)]
pub struct Checkpoint {
    pub n: u32,
    pub commit: String,
    pub time: String,
    pub message: String,
    pub actor: String,
}

/// Commit everything not ignored and tag it as the next checkpoint. Returns
/// `None` if nothing changed since the last checkpoint.
pub fn checkpoint(root: &Path, message: &str, actor: &str) -> Result<Option<Checkpoint>> {
    init(root)?;
    git(root, &["add", "-A"])?;
    let changed = !git(root, &["status", "--porcelain"])?.trim().is_empty();
    let has_head = git(root, &["rev-parse", "--verify", "HEAD"]).is_ok();
    let n = list(root)?.iter().map(|c| c.n).max().unwrap_or(0) + 1;
    if changed || !has_head {
        let msg = format!("{message}\n\nGamma-Actor: {actor}\nGamma-Checkpoint: {n}");
        let mut args: Vec<String> = identity_args(root);
        args.extend(["commit".into(), "-q".into(), "--allow-empty".into(), "-m".into(), msg]);
        let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        git(root, &refs)?;
    } else {
        // nothing new: tag HEAD only if it is not a checkpoint already
        let head = git(root, &["rev-parse", "HEAD"])?.trim().to_string();
        if list(root)?.iter().any(|c| c.commit == head) {
            return Ok(None);
        }
    }
    let tag = format!("gamma/checkpoint/{n}");
    git(root, &["tag", &tag])?;
    Ok(list(root)?.into_iter().find(|c| c.n == n))
}

pub fn list(root: &Path) -> Result<Vec<Checkpoint>> {
    if !is_repo(root) {
        return Ok(vec![]);
    }
    let out = git(root, &["for-each-ref", "refs/tags/gamma/checkpoint/", "--format=%(refname:short)%09%(*objectname)%(objectname)%09%(creatordate:iso-strict)%09%(contents:subject)%09%(contents)"]);
    let Ok(out) = out else { return Ok(vec![]) };
    let mut v = vec![];
    for line in out.lines() {
        let f: Vec<&str> = line.splitn(5, '\t').collect();
        if f.len() < 4 {
            continue;
        }
        let Some(n) = f[0].rsplit('/').next().and_then(|s| s.parse().ok()) else { continue };
        let commit = git(root, &["rev-list", "-n", "1", f[0]]).map(|s| s.trim().to_string()).unwrap_or_default();
        let body = git(root, &["log", "-1", "--format=%B", &commit]).unwrap_or_default();
        let actor = body.lines().find_map(|l| l.strip_prefix("Gamma-Actor: ")).unwrap_or("person").to_string();
        v.push(Checkpoint { n, commit, time: f[2].into(), message: f[3].into(), actor });
    }
    v.sort_by_key(|c| c.n);
    Ok(v)
}

/// A file as it was at a commit.
pub fn show(root: &Path, commit: &str, rel: &str) -> Result<Vec<u8>> {
    let out = Command::new("git").arg("-C").arg(root).args(["show", &format!("{commit}:{rel}")]).output()?;
    if !out.status.success() {
        bail!("{rel} is not in commit {commit}");
    }
    Ok(out.stdout)
}

/// Files changed between two commits.
pub fn changed_files(root: &Path, a: &str, b: &str) -> Result<Vec<String>> {
    Ok(git(root, &["diff", "--name-status", a, b])?.lines().map(|s| s.to_string()).collect())
}

pub fn text_diff(root: &Path, a: &str, b: &str) -> Result<String> {
    git(root, &["diff", "--stat", "--patch", "--no-color", a, b])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkpoints() {
        if !available() {
            return;
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "1").unwrap();
        let c1 = checkpoint(d.path(), "first", "person").unwrap().unwrap();
        assert_eq!(c1.n, 1);
        assert!(checkpoint(d.path(), "same", "person").unwrap().is_none());
        std::fs::write(d.path().join("a.txt"), "2").unwrap();
        let c2 = checkpoint(d.path(), "second", "agent:x").unwrap().unwrap();
        assert_eq!(c2.n, 2);
        assert_eq!(c2.actor, "agent:x");
        assert_eq!(show(d.path(), &c1.commit, "a.txt").unwrap(), b"1");
        assert_eq!(list(d.path()).unwrap().len(), 2);
    }
}
