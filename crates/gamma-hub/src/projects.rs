//! An open project: its manifest, caches of the layout, stack and
//! simulations, the event log, builds, runs, selection and comments.

use crate::events::EventLog;
use anyhow::{Context, Result, bail};
use gamma_core::layout::Layout;
use gamma_core::project::Project;
use gamma_core::stack::Stack;
use gamma_sim::Simulation;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Default)]
pub struct Cache {
    pub layout: Option<Option<Arc<Layout>>>,
    pub stack: Option<Option<Arc<Stack>>>,
    pub sims: HashMap<(String, bool), Arc<Simulation>>,
    pub layout_error: Option<String>,
}

pub struct ProjectState {
    pub id: String,
    pub project: RwLock<Project>,
    pub log: EventLog,
    pub cache: Mutex<Cache>,
    pub selection: Mutex<Option<Value>>,
    pub build_lock: tokio::sync::Mutex<()>,
    pub autobuild: std::sync::atomic::AtomicBool,
    pub watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

pub fn project_id(root: &Path) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(&Sha256::digest(root.to_string_lossy().as_bytes())[..6])
}

#[derive(Clone, Debug, Serialize)]
pub struct RunInfo {
    pub sim: String,
    pub id: String,
    pub run: Value,
    pub has_results: bool,
}

impl ProjectState {
    pub fn open(root: &Path) -> Result<Arc<ProjectState>> {
        let project = Project::open(root)?;
        let log = EventLog::open(project.local_dir().join("events.jsonl"))?;
        Ok(Arc::new(ProjectState {
            id: project_id(&project.root),
            project: RwLock::new(project),
            log,
            cache: Mutex::new(Cache::default()),
            selection: Mutex::new(None),
            build_lock: tokio::sync::Mutex::new(()),
            autobuild: std::sync::atomic::AtomicBool::new(true),
            watcher: Mutex::new(None),
        }))
    }

    pub fn root(&self) -> PathBuf {
        self.project.read().unwrap().root.clone()
    }

    pub fn snapshot(&self) -> Project {
        self.project.read().unwrap().clone()
    }

    pub fn reload_manifest(&self) -> Result<()> {
        let root = self.root();
        let p = Project::open(&root)?;
        *self.project.write().unwrap() = p;
        self.invalidate_all();
        Ok(())
    }

    pub fn invalidate_all(&self) {
        *self.cache.lock().unwrap() = Cache::default();
    }

    pub fn invalidate_layout(&self) {
        let mut c = self.cache.lock().unwrap();
        c.layout = None;
        c.layout_error = None;
    }

    pub fn invalidate_stack(&self) {
        self.cache.lock().unwrap().stack = None;
    }

    pub fn invalidate_sims(&self) {
        self.cache.lock().unwrap().sims.clear();
    }

    pub fn layout(&self) -> Result<Option<Arc<Layout>>> {
        if let Some(l) = self.cache.lock().unwrap().layout.clone() {
            return Ok(l);
        }
        let p = self.snapshot();
        match Layout::of_project(&p) {
            Ok(l) => {
                let l = l.map(Arc::new);
                self.cache.lock().unwrap().layout = Some(l.clone());
                Ok(l)
            }
            Err(e) => {
                self.cache.lock().unwrap().layout_error = Some(format!("{e:#}"));
                Err(e)
            }
        }
    }

    pub fn stack(&self) -> Option<Arc<Stack>> {
        if let Some(s) = self.cache.lock().unwrap().stack.clone() {
            return s;
        }
        let p = self.snapshot();
        let path = p.manifest.layout.as_ref().map(|l| p.path(&l.stack)).unwrap_or_else(|| p.root.join("stack.toml"));
        let s = Stack::load(&path).ok().map(Arc::new);
        self.cache.lock().unwrap().stack = Some(s.clone());
        s
    }

    pub fn sim(&self, name: &str, preview: bool) -> Result<Arc<Simulation>> {
        if let Some(s) = self.cache.lock().unwrap().sims.get(&(name.to_string(), preview)) {
            return Ok(s.clone());
        }
        let p = self.snapshot();
        let cfg = p.sim(name)?;
        let path = p.path(&cfg.spec);
        if !path.is_file() {
            bail!("simulation {name}: {} does not exist yet{}", cfg.spec, if cfg.generator.is_some() { "; build it first" } else { "" });
        }
        let mut sim = gamma_sim::setup::load(&path)?;
        if preview {
            let pv = cfg.preview.clone().unwrap_or(gamma_core::project::PreviewCfg { mesh_scale: 2.0, run_time_scale: 1.0 });
            sim = gamma_sim::setup::preview(&sim, pv.mesh_scale, pv.run_time_scale);
        }
        let sim = Arc::new(sim);
        self.cache.lock().unwrap().sims.insert((name.to_string(), preview), sim.clone());
        Ok(sim)
    }

    /// Run a generator script with the project's interpreter.
    pub async fn run_generator(&self, script: &str, actor: &str, what: &str) -> Result<Value> {
        let p = self.snapshot();
        let py = p.python();
        let t0 = std::time::Instant::now();
        self.log.push(actor, "build.started", json!({"target": what, "script": script}));
        let mut cmd = tokio::process::Command::new(&py);
        cmd.arg(script).current_dir(&p.root).env("GAMMA_PROJECT", &p.root).env("PYTHONUNBUFFERED", "1").kill_on_drop(true);
        let out = tokio::time::timeout(std::time::Duration::from_secs(1800), cmd.output())
            .await
            .map_err(|_| anyhow::anyhow!("{script} did not finish within 30 minutes"))?
            .with_context(|| format!("running {} {script} (set [python] interpreter in gamma.toml)", py.display()))?;
        let tail = |b: &[u8]| -> String {
            let s = String::from_utf8_lossy(b);
            let lines: Vec<&str> = s.lines().collect();
            lines[lines.len().saturating_sub(40)..].join("\n")
        };
        let res = json!({"target": what, "script": script, "ok": out.status.success(), "seconds": t0.elapsed().as_secs_f64(), "stdout": tail(&out.stdout), "stderr": tail(&out.stderr)});
        self.log.push(actor, "build.done", res.clone());
        if !out.status.success() {
            bail!("{script} failed ({}):\n{}", out.status, tail(&out.stderr));
        }
        Ok(res)
    }

    /// Build the layout and/or simulation specs. `target`: `all`, `layout` or `sim:<name>`.
    pub async fn build(&self, target: &str, actor: &str) -> Result<Vec<Value>> {
        let _g = self.build_lock.lock().await;
        let p = self.snapshot();
        let mut out = vec![];
        if target == "all" || target == "layout" {
            if let Some(g) = p.manifest.layout.as_ref().and_then(|l| l.generator.clone()) {
                out.push(self.run_generator(&g, actor, "layout").await?);
                self.invalidate_layout();
                self.log.push("hub", "layout.changed", json!({}));
            }
        }
        for s in &p.manifest.simulations {
            if target == "all" || target == format!("sim:{}", s.name) {
                if let Some(g) = &s.generator {
                    out.push(self.run_generator(g, actor, &format!("sim:{}", s.name)).await?);
                }
            }
        }
        self.invalidate_sims();
        if target != "layout" {
            self.log.push("hub", "sim.changed", json!({"target": target}));
        }
        Ok(out)
    }

    pub fn runs(&self, sim: Option<&str>) -> Vec<RunInfo> {
        let p = self.snapshot();
        let mut out = vec![];
        let base = p.root.join("results");
        let Ok(sims) = std::fs::read_dir(&base) else { return out };
        for s in sims.flatten() {
            let sname = s.file_name().to_string_lossy().to_string();
            if sim.map(|x| x != sname).unwrap_or(false) {
                continue;
            }
            let Ok(runs) = std::fs::read_dir(s.path()) else { continue };
            for r in runs.flatten() {
                let rp = r.path();
                let Ok(t) = std::fs::read_to_string(rp.join("run.json")) else { continue };
                let run: Value = serde_json::from_str(&t).unwrap_or(json!({}));
                out.push(RunInfo { sim: sname.clone(), id: r.file_name().to_string_lossy().to_string(), has_results: rp.join("results.json").is_file(), run });
            }
        }
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out
    }

    pub fn run_dir(&self, sim: &str, id: &str) -> Result<PathBuf> {
        if sim.contains("..") || id.contains("..") || id.contains('/') || sim.contains('/') {
            bail!("bad run reference");
        }
        let d = self.root().join("results").join(sim).join(id);
        if !d.is_dir() {
            bail!("no run {sim}/{id}");
        }
        Ok(d)
    }

    /// The spec a run used (its copy, else the project's current spec).
    pub fn run_spec(&self, sim: &str, id: &str) -> Result<Simulation> {
        let d = self.run_dir(sim, id)?;
        let copy = d.join("spec.json");
        if copy.is_file() {
            return gamma_sim::setup::load(&copy);
        }
        Ok((*self.sim(sim, false)?).clone())
    }

    pub fn comments_path(&self) -> PathBuf {
        self.root().join(".gamma").join("comments.jsonl")
    }

    pub fn comments(&self) -> Vec<Value> {
        gamma_core::fsutil::read_lines_repair(&self.comments_path()).unwrap_or_default().iter().filter_map(|l| serde_json::from_str(l).ok()).collect()
    }

    pub fn add_comment(&self, actor: &str, text: &str, object: Option<&str>, checkpoint: Option<u32>) -> Result<Value> {
        let c = json!({"time": crate::events::now(), "actor": actor, "text": text, "object": object, "checkpoint": checkpoint, "id": format!("c{}", rand::random::<u32>())});
        gamma_core::fsutil::append_line(&self.comments_path(), &c.to_string())?;
        self.log.push(actor, "comment", c.clone());
        Ok(c)
    }

    /// Relative path inside the project, refusing escapes.
    pub fn safe_path(&self, rel: &str) -> Result<PathBuf> {
        let root = self.root();
        let p = Path::new(rel);
        if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            bail!("path must be relative to the project and stay inside it: {rel}");
        }
        Ok(root.join(p))
    }

    pub fn info(&self) -> Value {
        let p = self.snapshot();
        json!({
            "id": self.id,
            "root": p.root,
            "name": p.manifest.project.name,
            "description": p.manifest.project.description,
            "manifest": p.manifest,
            "autobuild": self.autobuild.load(std::sync::atomic::Ordering::Relaxed),
            "layout_error": self.cache.lock().unwrap().layout_error,
        })
    }
}

/// Files of the project for the tree (skipping build outputs, caches and git).
pub fn file_tree(root: &Path) -> Vec<Value> {
    let mut out = vec![];
    fn walk(base: &Path, dir: &Path, out: &mut Vec<Value>, depth: usize) {
        if depth > 6 || out.len() > 2000 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') && name != ".gamma" || name == "__pycache__" || name == "node_modules" || name == "local" && dir.ends_with(".gamma") {
                continue;
            }
            let p = e.path();
            let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().to_string();
            let is_dir = p.is_dir();
            let size = if is_dir { 0 } else { e.metadata().map(|m| m.len()).unwrap_or(0) };
            out.push(json!({"path": rel, "dir": is_dir, "size": size}));
            if is_dir {
                walk(base, &p, out, depth + 1);
            }
        }
    }
    walk(root, root, &mut out, 0);
    out
}

/// Keep the layout of a checkpoint (the GDS is a generated, git-ignored file)
/// so checkpoints can be compared later.
pub fn save_checkpoint_layout(ps: &ProjectState, n: u32) -> Result<()> {
    let p = ps.snapshot();
    let Some(l) = &p.manifest.layout else { return Ok(()) };
    let dir = p.local_dir().join("checkpoints").join(n.to_string());
    std::fs::create_dir_all(&dir)?;
    let gds = p.path(&l.gds);
    if gds.is_file() {
        std::fs::copy(&gds, dir.join("layout.gds"))?;
    }
    if let Some(pp) = &l.ports {
        let src = p.path(pp);
        if src.is_file() {
            std::fs::copy(src, dir.join("ports.json"))?;
        }
    }
    Ok(())
}

pub fn checkpoint_layout(ps: &ProjectState, n: u32) -> Result<Option<Layout>> {
    let p = ps.snapshot();
    let dir = p.local_dir().join("checkpoints").join(n.to_string());
    let gds = dir.join("layout.gds");
    if !gds.is_file() {
        return Ok(None);
    }
    let ports = dir.join("ports.json");
    let top = p.manifest.layout.as_ref().and_then(|l| l.top.clone());
    Layout::read(&gds, top.as_deref(), if ports.is_file() { Some(ports.as_path()) } else { None }).map(Some)
}

/// Create a new project folder: manifest, stack, folders, .gitignore, git.
pub fn init_project(root: &Path, name: Option<&str>) -> Result<()> {
    std::fs::create_dir_all(root)?;
    let mf = root.join(gamma_core::project::MANIFEST);
    if mf.exists() {
        return Ok(());
    }
    let name = name.map(|s| s.to_string()).unwrap_or_else(|| root.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "project".into()));
    let mut m = gamma_core::Manifest::new(&name);
    m.layout = Some(gamma_core::project::LayoutCfg { gds: "build/layout.gds".into(), generator: None, ports: None, top: None, stack: "stack.toml".into() });
    gamma_core::fsutil::atomic_write(&mf, toml::to_string_pretty(&m)?.as_bytes())?;
    let st = root.join("stack.toml");
    if !st.exists() {
        gamma_core::fsutil::atomic_write(&st, gamma_core::Stack::soi220().to_toml().as_bytes())?;
    }
    for d in ["layout", "sims", "circuits", "results"] {
        std::fs::create_dir_all(root.join(d))?;
    }
    let gi = root.join(".gitignore");
    if !gi.exists() {
        std::fs::write(gi, gamma_core::project::PROJECT_GITIGNORE)?;
    }
    if crate::git::available() {
        crate::git::init(root)?;
    }
    Ok(())
}

/// Compare checkpoint `a` with checkpoint `b` (or the working state).
pub fn diff_checkpoints(ps: &ProjectState, a: u32, b: Option<u32>) -> Result<Value> {
    let root = ps.root();
    let list = crate::git::list(&root)?;
    let ca = list.iter().find(|c| c.n == a).with_context(|| format!("no checkpoint {a}"))?;
    let cb = match b {
        Some(b) => Some(list.iter().find(|c| c.n == b).with_context(|| format!("no checkpoint {b}"))?),
        None => None,
    };
    let la = checkpoint_layout(ps, a)?;
    let lb = match b {
        Some(b) => checkpoint_layout(ps, b)?,
        None => ps.layout()?.map(|l| (*l).clone()),
    };
    let layout = match (&la, &lb) {
        (Some(x), Some(y)) => json!(gamma_core::layout::diff(x, y)),
        _ => json!(null),
    };
    let files = match cb {
        Some(cb) => crate::git::changed_files(&root, &ca.commit, &cb.commit)?,
        None => crate::git::changed_files(&root, &ca.commit, "HEAD").unwrap_or_default(),
    };
    // result diff: latest run metrics of each simulation at both checkpoints
    let metrics_at = |commit: Option<&str>| -> HashMap<String, Value> {
        let mut out = HashMap::new();
        let Ok(rd) = std::fs::read_dir(root.join("results")) else { return out };
        for s in rd.flatten() {
            let sim = s.file_name().to_string_lossy().to_string();
            let mut runs: Vec<String> = std::fs::read_dir(s.path()).map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
            runs.sort();
            for run in runs.iter().rev() {
                let rel = format!("results/{sim}/{run}/run.json");
                let text = match commit {
                    Some(c) => crate::git::show(&root, c, &rel).ok().map(|b| String::from_utf8_lossy(&b).to_string()),
                    None => std::fs::read_to_string(root.join(&rel)).ok(),
                };
                if let Some(t) = text {
                    if let Ok(v) = serde_json::from_str::<Value>(&t) {
                        if v["status"] == "done" {
                            out.insert(sim.clone(), json!({"run": run, "metrics": v["metrics"]}));
                            break;
                        }
                    }
                }
            }
        }
        out
    };
    let ma = metrics_at(Some(&ca.commit));
    let mb = metrics_at(cb.map(|c| c.commit.as_str()));
    let mut results = vec![];
    let mut sims: Vec<&String> = ma.keys().chain(mb.keys()).collect();
    sims.sort();
    sims.dedup();
    for s in sims {
        let x = ma.get(s).cloned().unwrap_or(json!({}));
        let y = mb.get(s).cloned().unwrap_or(json!({}));
        results.push(json!({"sim": s, "before": x["run"], "after": y["run"], "metrics": gamma_sim::results::diff_metrics(&x["metrics"], &y["metrics"])}));
    }
    Ok(json!({"a": ca, "b": cb, "files": files, "layout": layout, "results": results, "layout_note": if la.is_none() { "the layout of the first checkpoint was not kept on this machine" } else { "" }}))
}
