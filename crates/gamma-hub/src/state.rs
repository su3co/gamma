//! The hub's state.

use crate::config::UserConfig;
use crate::jobs::Jobs;
use crate::projects::ProjectState;
use anyhow::{Result, bail};
use gamma_core::project::RemoteCfg;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct Inner {
    pub token: String,
    pub projects: Mutex<HashMap<String, Arc<ProjectState>>>,
    pub jobs: Arc<Jobs>,
    pub config: Mutex<UserConfig>,
    /// The `gamma` program used to start workers (this program by default).
    pub worker: PathBuf,
    /// Folder of the built interface served to browsers, if any.
    pub ui_dir: Option<PathBuf>,
    pub started: std::time::Instant,
    pub agents: crate::agent::Agents,
    pub devices: Vec<gamma_sim::devices::DeviceInfo>,
    pub throughput: Mutex<HashMap<String, f64>>,
}

#[derive(Clone)]
pub struct Hub(pub Arc<Inner>);

impl std::ops::Deref for Hub {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

fn throughput_path() -> PathBuf {
    crate::config::data_dir().join("throughput.json")
}

impl Hub {
    pub fn new(token: String, worker: PathBuf, ui_dir: Option<PathBuf>) -> Hub {
        let tp: HashMap<String, f64> = std::fs::read_to_string(throughput_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Hub(Arc::new(Inner {
            token,
            projects: Mutex::new(HashMap::new()),
            jobs: Arc::new(Jobs::default()),
            config: Mutex::new(UserConfig::load()),
            worker,
            ui_dir,
            started: std::time::Instant::now(),
            agents: crate::agent::Agents::default(),
            devices: gamma_sim::devices::local_devices(),
            throughput: Mutex::new(tp),
        }))
    }

    pub fn open(&self, root: &Path) -> Result<Arc<ProjectState>> {
        let root = root.canonicalize().map_err(|e| anyhow::anyhow!("cannot open {}: {e}", root.display()))?;
        let id = crate::projects::project_id(&root);
        if let Some(p) = self.projects.lock().unwrap().get(&id) {
            return Ok(p.clone());
        }
        let ps = ProjectState::open(&root)?;
        crate::watch::start(self.clone(), ps.clone())?;
        self.projects.lock().unwrap().insert(id.clone(), ps.clone());
        ps.log.push("hub", "project.opened", json!({"root": root}));
        remember_recent(&root);
        Ok(ps)
    }

    pub fn project(&self, id: &str) -> Result<Arc<ProjectState>> {
        match self.projects.lock().unwrap().get(id) {
            Some(p) => Ok(p.clone()),
            None => bail!("no open project with id {id}; open it first"),
        }
    }

    /// Remotes from the user's config and the project manifest (project wins on name clashes).
    pub fn remotes(&self, ps: Option<&ProjectState>) -> Vec<RemoteCfg> {
        let mut v: Vec<RemoteCfg> = self.config.lock().unwrap().remotes.clone();
        if let Some(ps) = ps {
            for r in ps.snapshot().manifest.remotes {
                v.retain(|x| x.name != r.name);
                v.push(r);
            }
        }
        v
    }

    pub fn remote(&self, name: &str, ps: Option<&ProjectState>) -> Result<RemoteCfg> {
        let all = self.remotes(ps);
        match all.iter().find(|r| r.name == name) {
            Some(r) => Ok(r.clone()),
            None => bail!("no remote host named '{name}'; known: {}", if all.is_empty() { "none (add one with `gamma remote add`)".into() } else { all.iter().map(|r| r.name.clone()).collect::<Vec<_>>().join(", ") }),
        }
    }

    /// Devices with the measured throughput where there is one.
    pub fn devices(&self) -> Vec<gamma_sim::devices::DeviceInfo> {
        let tp = self.throughput.lock().unwrap();
        self.devices
            .iter()
            .map(|d| {
                let mut d = d.clone();
                if let Some(t) = tp.get(&d.name) {
                    d.throughput = *t;
                }
                d
            })
            .collect()
    }

    /// Record a measured throughput (cells/s) for a device name (moving average).
    pub fn record_throughput(&self, device_name: &str, cells_per_s: f64) {
        if !(cells_per_s.is_finite() && cells_per_s > 0.0) {
            return;
        }
        let mut tp = self.throughput.lock().unwrap();
        let e = tp.entry(device_name.to_string()).or_insert(cells_per_s);
        *e = 0.5 * *e + 0.5 * cells_per_s;
        let _ = gamma_core::fsutil::atomic_write(&throughput_path(), serde_json::to_string_pretty(&*tp).unwrap_or_default().as_bytes());
    }

    pub fn info(&self) -> Value {
        json!({
            "version": gamma_core::VERSION,
            "lightsim": gamma_sim::LIGHTSIM_REV,
            "pid": std::process::id(),
            "uptime_s": self.started.elapsed().as_secs(),
            "projects": self.projects.lock().unwrap().values().map(|p| p.info()).collect::<Vec<_>>(),
            "devices": self.devices(),
            "precision": gamma_sim::devices::precision(),
            "cuda": cfg!(feature = "cuda"),
            "jobs_active": self.jobs.active(),
            "platform": format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
            "recent": recent(),
        })
    }
}

fn recent_path() -> PathBuf {
    crate::config::data_dir().join("recent.json")
}

pub fn recent() -> Vec<String> {
    std::fs::read_to_string(recent_path()).ok().and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok()).unwrap_or_default().into_iter().filter(|p| Path::new(p).join("gamma.toml").is_file()).collect()
}

fn remember_recent(root: &Path) {
    let mut r = recent();
    let s = root.to_string_lossy().to_string();
    r.retain(|x| x != &s);
    r.insert(0, s);
    r.truncate(12);
    let _ = gamma_core::fsutil::atomic_write(&recent_path(), serde_json::to_string(&r).unwrap_or_default().as_bytes());
}
