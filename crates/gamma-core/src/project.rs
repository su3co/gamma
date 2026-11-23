//! The project folder and its manifest, `gamma.toml`.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MANIFEST: &str = "gamma.toml";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub project: ProjectMeta,
    #[serde(default)]
    pub python: PythonCfg,
    #[serde(default)]
    pub layout: Option<LayoutCfg>,
    #[serde(default, rename = "simulation")]
    pub simulations: Vec<SimCfg>,
    #[serde(default, rename = "circuit")]
    pub circuits: Vec<CircuitCfg>,
    #[serde(default, rename = "remote")]
    pub remotes: Vec<RemoteCfg>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectMeta {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PythonCfg {
    /// Interpreter used to run generators, relative to the project or on PATH.
    #[serde(default = "default_python")]
    pub interpreter: String,
}

fn default_python() -> String {
    "python3".into()
}

impl Default for PythonCfg {
    fn default() -> Self {
        PythonCfg {
            interpreter: default_python(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LayoutCfg {
    /// GDS file the viewers read (usually generated).
    pub gds: String,
    /// Script that writes the GDS (and ports), run with the project interpreter.
    #[serde(default)]
    pub generator: Option<String>,
    /// Ports JSON written by the generator (`[{name, x, y, width, orientation, layer}]`).
    #[serde(default)]
    pub ports: Option<String>,
    /// Top cell; default: the cell no other cell references (largest if several).
    #[serde(default)]
    pub top: Option<String>,
    /// Layer stack file.
    #[serde(default = "default_stack")]
    pub stack: String,
}

fn default_stack() -> String {
    "stack.toml".into()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SimCfg {
    pub name: String,
    /// lightsim simulation file.
    pub spec: String,
    /// Script that writes the spec.
    #[serde(default)]
    pub generator: Option<String>,
    /// Default device (`cpu`, `cuda:0`, `remote:<name>/cuda:0`).
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub description: String,
    /// Coarse variant for quick runs.
    #[serde(default)]
    pub preview: Option<PreviewCfg>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PreviewCfg {
    /// Multiply the mesh spacing by this factor.
    #[serde(default = "one")]
    pub mesh_scale: f64,
    /// Multiply the run time by this factor.
    #[serde(default = "one")]
    pub run_time_scale: f64,
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CircuitCfg {
    pub name: String,
    pub file: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct RemoteCfg {
    pub name: String,
    /// `user@host` or an SSH config alias.
    pub ssh: String,
    /// Optional command prefix put before the worker command (a job queue).
    /// `{label}` and `{gpus}` are replaced.
    #[serde(default)]
    pub launcher: Option<String>,
    /// Path of the gamma program on the host; default: the installed one.
    #[serde(default)]
    pub program: Option<String>,
    /// Folder on the host where jobs are staged; default `~/.cache/gamma/jobs`.
    #[serde(default)]
    pub workdir: Option<String>,
    /// The launcher makes only the GPUs it grants visible to the job
    /// (`CUDA_VISIBLE_DEVICES`), so the worker numbers them from `cuda:0`.
    #[serde(default)]
    pub renumber_gpus: bool,
}

/// An opened project: its root folder and manifest.
#[derive(Clone, Debug)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: Manifest,
}

impl Project {
    /// Open the project whose root is `dir` or one of its parents.
    pub fn discover(dir: &Path) -> Result<Project> {
        let dir = dir
            .canonicalize()
            .with_context(|| format!("no such folder: {}", dir.display()))?;
        let mut cur: Option<&Path> = Some(&dir);
        while let Some(d) = cur {
            if d.join(MANIFEST).is_file() {
                return Project::open(d);
            }
            cur = d.parent();
        }
        bail!(
            "{} is not inside a gamma project (no {MANIFEST} here or above); create one with `gamma init`",
            dir.display()
        )
    }

    pub fn open(root: &Path) -> Result<Project> {
        let root = root
            .canonicalize()
            .with_context(|| format!("no such folder: {}", root.display()))?;
        let p = root.join(MANIFEST);
        let text =
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        let manifest: Manifest = toml::from_str(&text)
            .with_context(|| format!("{} is not a valid gamma manifest", p.display()))?;
        manifest.validate()?;
        Ok(Project { root, manifest })
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        let p = Path::new(rel);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    pub fn sim(&self, name: &str) -> Result<&SimCfg> {
        self.manifest
            .simulations
            .iter()
            .find(|s| s.name == name)
            .ok_or_else(|| {
                let names: Vec<&str> = self
                    .manifest
                    .simulations
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect();
                anyhow::anyhow!(
                    "no simulation named '{name}' in {MANIFEST}; simulations: {}",
                    if names.is_empty() {
                        "none".into()
                    } else {
                        names.join(", ")
                    }
                )
            })
    }

    /// Folder holding gamma's local state (event log, caches): git-ignored.
    pub fn local_dir(&self) -> PathBuf {
        self.root.join(".gamma").join("local")
    }

    pub fn results_dir(&self, sim: &str) -> PathBuf {
        self.root.join("results").join(sim)
    }

    pub fn save_manifest(&self) -> Result<()> {
        let text = toml::to_string_pretty(&self.manifest)?;
        crate::fsutil::atomic_write(&self.root.join(MANIFEST), text.as_bytes())
    }

    /// The interpreter for generators: a relative path is taken from the project root.
    pub fn python(&self) -> PathBuf {
        let i = &self.manifest.python.interpreter;
        if i.contains('/') || i.contains('\\') {
            self.path(i)
        } else {
            PathBuf::from(i)
        }
    }
}

impl Manifest {
    pub fn validate(&self) -> Result<()> {
        if self.project.name.trim().is_empty() {
            bail!("[project] name is empty");
        }
        let mut seen = std::collections::BTreeSet::new();
        for s in &self.simulations {
            if s.name.is_empty() || s.name.contains('/') || s.name.contains(' ') {
                bail!(
                    "simulation name '{}' must be non-empty and contain no '/' or spaces",
                    s.name
                );
            }
            if !seen.insert(&s.name) {
                bail!("two simulations are named '{}'", s.name);
            }
        }
        Ok(())
    }

    pub fn new(name: &str) -> Manifest {
        Manifest {
            project: ProjectMeta {
                name: name.into(),
                description: String::new(),
            },
            ..Default::default()
        }
    }
}

/// The `.gitignore` gamma writes into a new project.
pub const PROJECT_GITIGNORE: &str = "build/\n.gamma/local/\n__pycache__/\n*.pyc\n.venv/\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_manifest() {
        let m: Manifest = toml::from_str(
            r#"
[project]
name = "Ring"
[layout]
gds = "build/ring.gds"
generator = "layout/ring.py"
[[simulation]]
name = "ringdown"
spec = "build/sims/ringdown.json"
[simulation.preview]
mesh_scale = 2.0
[[remote]]
name = "gpu"
ssh = "me@host"
"#,
        )
        .unwrap();
        m.validate().unwrap();
        assert_eq!(m.simulations[0].preview.as_ref().unwrap().mesh_scale, 2.0);
        assert_eq!(m.layout.unwrap().stack, "stack.toml");
        assert_eq!(m.python.interpreter, "python3");
    }
}
