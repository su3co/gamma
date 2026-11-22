//! # gamma-quantum
//!
//! The integration point for **lightsim-quantum**, the library for quantum
//! light in photonic circuits (the counterpart of Lumerical qINTERCONNECT).
//!
//! lightsim-quantum has no stable API yet. Until it does, gamma:
//!
//! * lists the circuits a project names (`[[circuit]]` in `gamma.toml`),
//! * reads a circuit file as generic JSON and extracts what a circuit view
//!   needs if the keys are present (`components`/`elements`, `connections`/
//!   `links`, `sources`, `detectors`),
//! * runs a circuit through an external program when one is installed: the
//!   command is a template, by default `lightsim-quantum run {circuit} --out
//!   {results}`, overridable with `GAMMA_LIGHTSIM_QUANTUM` (provisional:
//!   it will follow the library's CLI once that is published).
//!
//! [`Engine`] is the seam: an in-process engine linking the library replaces
//! [`ExternalEngine`] when the Rust API exists.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;

#[derive(Clone, Debug, Serialize)]
pub struct CircuitView {
    pub name: String,
    pub components: Vec<Node>,
    pub connections: Vec<(String, String)>,
    /// Keys at the top level of the file, for display when the format is not recognised.
    pub keys: Vec<String>,
    pub recognised: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
}

pub fn read_circuit(path: &Path) -> Result<CircuitView> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let v: Value =
        serde_json::from_str(&text).with_context(|| format!("{} is not JSON", path.display()))?;
    Ok(view(
        &v,
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("circuit"),
    ))
}

pub fn view(v: &Value, fallback_name: &str) -> CircuitView {
    let keys = v
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let list = v
        .get("components")
        .or_else(|| v.get("elements"))
        .or_else(|| v.get("instances"));
    let mut components = vec![];
    if let Some(l) = list {
        if let Some(a) = l.as_array() {
            for (i, c) in a.iter().enumerate() {
                components.push(Node {
                    id: c
                        .get("id")
                        .or_else(|| c.get("name"))
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("c{i}")),
                    kind: c
                        .get("type")
                        .or_else(|| c.get("kind"))
                        .or_else(|| c.get("component"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("?")
                        .to_string(),
                });
            }
        } else if let Some(o) = l.as_object() {
            for (k, c) in o {
                components.push(Node {
                    id: k.clone(),
                    kind: c
                        .get("type")
                        .or_else(|| c.get("component"))
                        .and_then(|x| x.as_str())
                        .unwrap_or("?")
                        .to_string(),
                });
            }
        }
    }
    let mut connections = vec![];
    if let Some(l) = v.get("connections").or_else(|| v.get("links")) {
        if let Some(a) = l.as_array() {
            for c in a {
                if let Some(p) = c.as_array() {
                    if p.len() >= 2 {
                        connections.push((
                            p[0].as_str().unwrap_or("").to_string(),
                            p[1].as_str().unwrap_or("").to_string(),
                        ));
                    }
                } else if let (Some(a), Some(b)) = (
                    c.get("from").and_then(|x| x.as_str()),
                    c.get("to").and_then(|x| x.as_str()),
                ) {
                    connections.push((a.into(), b.into()));
                }
            }
        } else if let Some(o) = l.as_object() {
            for (a, b) in o {
                connections.push((a.clone(), b.as_str().unwrap_or("").to_string()));
            }
        }
    }
    CircuitView {
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(fallback_name)
            .to_string(),
        recognised: !components.is_empty(),
        components,
        connections,
        keys,
    }
}

/// Runs circuits.
pub trait Engine {
    fn name(&self) -> String;
    fn available(&self) -> bool;
    fn run(&self, circuit: &Path, results: &Path) -> Result<()>;
}

/// lightsim-quantum through its command-line program.
pub struct ExternalEngine {
    pub template: String,
}

impl Default for ExternalEngine {
    fn default() -> Self {
        ExternalEngine {
            template: std::env::var("GAMMA_LIGHTSIM_QUANTUM")
                .unwrap_or_else(|_| "lightsim-quantum run {circuit} --out {results}".into()),
        }
    }
}

impl Engine for ExternalEngine {
    fn name(&self) -> String {
        self.template
            .split_whitespace()
            .next()
            .unwrap_or("lightsim-quantum")
            .to_string()
    }
    fn available(&self) -> bool {
        which(&self.name())
    }
    fn run(&self, circuit: &Path, results: &Path) -> Result<()> {
        if !self.available() {
            bail!(
                "lightsim-quantum is not installed (no '{}' on PATH); circuits can be viewed but not run yet",
                self.name()
            );
        }
        let parts: Vec<String> = self
            .template
            .split_whitespace()
            .map(|p| {
                p.replace("{circuit}", &circuit.to_string_lossy())
                    .replace("{results}", &results.to_string_lossy())
            })
            .collect();
        let st = std::process::Command::new(&parts[0])
            .args(&parts[1..])
            .status()?;
        if !st.success() {
            bail!("{} exited with {st}", parts[0]);
        }
        Ok(())
    }
}

fn which(prog: &str) -> bool {
    if prog.contains('/') {
        return Path::new(prog).is_file();
    }
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(prog).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generic_view() {
        let v: Value = serde_json::from_str(r#"{"name":"hom","components":[{"id":"bs","type":"beamsplitter"},{"id":"d1","type":"detector"}],"connections":[["bs.o1","d1.i"]]}"#).unwrap();
        let c = view(&v, "x");
        assert!(c.recognised);
        assert_eq!(c.components.len(), 2);
        assert_eq!(c.connections[0].0, "bs.o1");
        let u = view(&serde_json::json!({"foo": 1}), "x");
        assert!(!u.recognised);
        assert_eq!(u.keys, vec!["foo"]);
    }
}
