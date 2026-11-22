//! File watching: viewers follow edits made by the person, the agent or any
//! other program. Changes are debounced, classified, and turned into events;
//! edits to layout or simulation sources rebuild the generated files when
//! automatic builds are on.

use crate::projects::ProjectState;
use crate::state::Hub;
use anyhow::Result;
use notify::{EventKind, RecursiveMode, Watcher};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

#[derive(Debug, Default, PartialEq)]
pub struct Classified {
    pub manifest: bool,
    pub stack: bool,
    pub layout_output: bool,
    pub spec_output: bool,
    pub layout_source: bool,
    pub sim_source: bool,
    pub results: bool,
    pub files: Vec<String>,
}

pub fn classify(p: &gamma_core::Project, paths: &BTreeSet<PathBuf>) -> Classified {
    let mut c = Classified::default();
    let rel = |x: &Path| x.strip_prefix(&p.root).unwrap_or(x).to_string_lossy().replace('\\', "/");
    let layout = p.manifest.layout.clone().unwrap_or_default();
    let specs: Vec<String> = p.manifest.simulations.iter().map(|s| s.spec.clone()).collect();
    let sim_gens: Vec<String> = p.manifest.simulations.iter().filter_map(|s| s.generator.clone()).collect();
    for path in paths {
        let r = rel(path);
        if r.starts_with(".git/") || r.starts_with(".gamma/local/") || r.contains("__pycache__") || r.ends_with(".tmp") || r.contains("/.") && r.contains(".tmp-") {
            continue;
        }
        c.files.push(r.clone());
        if r == gamma_core::project::MANIFEST {
            c.manifest = true;
        } else if r == layout.stack {
            c.stack = true;
        } else if r == layout.gds || Some(&r) == layout.ports.as_ref() {
            c.layout_output = true;
        } else if specs.contains(&r) {
            c.spec_output = true;
        } else if r.starts_with("results/") {
            c.results = true;
        } else if r.starts_with("build/") {
            // other generated files
        } else if r.starts_with("sims/") || sim_gens.contains(&r) {
            c.sim_source = true;
        } else if r.ends_with(".py") || r.ends_with(".yml") || r.ends_with(".yaml") {
            c.layout_source = true;
        }
    }
    c
}

pub fn start(hub: Hub, ps: Arc<ProjectState>) -> Result<()> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            if matches!(ev.kind, EventKind::Access(_)) {
                return;
            }
            for p in ev.paths {
                let _ = tx.send(p);
            }
        }
    })?;
    w.watch(&ps.root(), RecursiveMode::Recursive)?;
    *ps.watcher.lock().unwrap() = Some(w);
    let weak = Arc::downgrade(&ps);
    tokio::spawn(async move {
        loop {
            let Some(first) = rx.recv().await else { break };
            let mut set = BTreeSet::new();
            set.insert(first);
            // debounce: collect until 250 ms of quiet
            loop {
                match tokio::time::timeout(Duration::from_millis(250), rx.recv()).await {
                    Ok(Some(p)) => {
                        set.insert(p);
                    }
                    _ => break,
                }
            }
            let Some(ps) = weak.upgrade() else { break };
            handle(&hub, &ps, &set).await;
        }
    });
    Ok(())
}

async fn handle(_hub: &Hub, ps: &Arc<ProjectState>, set: &BTreeSet<PathBuf>) {
    let p = ps.snapshot();
    let c = classify(&p, set);
    if c.files.is_empty() {
        return;
    }
    ps.log.push("hub", "files.changed", json!({"files": c.files}));
    if c.manifest {
        match ps.reload_manifest() {
            Ok(()) => {
                ps.log.push("hub", "project.changed", json!({}));
            }
            Err(e) => {
                ps.log.push("hub", "error", json!({"message": format!("{e:#}"), "file": "gamma.toml"}));
            }
        }
    }
    if c.stack {
        ps.invalidate_stack();
        ps.log.push("hub", "stack.changed", json!({}));
    }
    if c.layout_output {
        ps.invalidate_layout();
        ps.log.push("hub", "layout.changed", json!({}));
    }
    if c.spec_output {
        ps.invalidate_sims();
        ps.log.push("hub", "sim.changed", json!({}));
    }
    if c.results {
        ps.log.push("hub", "results.changed", json!({}));
    }
    if ps.autobuild.load(Ordering::Relaxed) && (c.layout_source || c.sim_source) {
        let target = if c.layout_source { "all" } else { "sims" };
        let ps2 = ps.clone();
        tokio::spawn(async move {
            let r = if target == "all" {
                ps2.build("all", "hub").await
            } else {
                let names: Vec<String> = ps2.snapshot().manifest.simulations.iter().map(|s| s.name.clone()).collect();
                let mut all = vec![];
                for n in names {
                    match ps2.build(&format!("sim:{n}"), "hub").await {
                        Ok(v) => all.extend(v),
                        Err(e) => {
                            ps2.log.push("hub", "error", json!({"message": format!("automatic build of sim:{n} failed: {e:#}")}));
                        }
                    }
                }
                Ok(all)
            };
            if let Err(e) = r {
                ps2.log.push("hub", "error", json!({"message": format!("automatic build failed: {e:#}")}));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classify_paths() {
        let m: gamma_core::Manifest = toml::from_str(
            r#"
[project]
name = "x"
[layout]
gds = "build/a.gds"
generator = "layout/a.py"
[[simulation]]
name = "s"
spec = "build/sims/s.json"
generator = "sims/s.py"
"#,
        )
        .unwrap();
        let p = gamma_core::Project { root: PathBuf::from("/p"), manifest: m };
        let set: BTreeSet<PathBuf> = ["/p/layout/a.py", "/p/build/a.gds", "/p/.git/index", "/p/sims/s.py", "/p/results/s/1/run.json"].iter().map(PathBuf::from).collect();
        let c = classify(&p, &set);
        assert!(c.layout_source && c.layout_output && c.sim_source && c.results && !c.manifest);
        assert_eq!(c.files.len(), 4);
    }
}
