//! # gamma-quantum
//!
//! gamma's integration of **lightsim-quantum**, the library for quantum light
//! in photonic circuits. gamma talks to it through its command-line program
//! (`lightsim-quantum run <document> --stream`), so gamma builds and runs
//! without it; circuits can be viewed always and run when the program is
//! installed (on PATH, next to gamma, or named by `GAMMA_LIGHTSIM_QUANTUM`).
//!
//! * [`read_circuit`] / [`view`] — what a circuit document contains
//!   (`lightsim-quantum/1`: `circuit` with instances, connections, ports;
//!   also the `modes` and `matrix` kinds), its tasks and sweep;
//! * [`ExternalEngine`] — runs a document, streaming progress;
//! * [`summarize`] — plots, bar charts and tables from the results (sweeps,
//!   HOM dips, output distributions, transfer matrices, Gaussian clicks).
//!
//! An in-process engine (the `lightsim_quantum::spec::run` API) replaces the
//! external program once lightsim-quantum is published.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct CircuitView {
    pub name: String,
    pub kind: String,
    pub components: Vec<Node>,
    pub connections: Vec<(String, String)>,
    pub ports: Vec<(String, String)>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub tasks: Vec<Node>,
    pub sweep: Option<String>,
    pub wavelengths: Vec<f64>,
    /// Keys at the top level of the file.
    pub keys: Vec<String>,
    pub recognised: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub id: String,
    pub kind: String,
    /// Parameters, as written.
    pub params: Value,
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

fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn pairs(v: &Value) -> Vec<(String, String)> {
    let mut out = vec![];
    if let Some(a) = v.as_array() {
        for c in a {
            if let Some(p) = c.as_array() {
                if p.len() >= 2 {
                    out.push((
                        p[0].as_str().unwrap_or("").to_string(),
                        p[1].as_str().unwrap_or("").to_string(),
                    ));
                }
            } else if let (Some(x), Some(y)) = (
                c.get("from").and_then(|x| x.as_str()),
                c.get("to").and_then(|x| x.as_str()),
            ) {
                out.push((x.into(), y.into()));
            }
        }
    } else if let Some(o) = v.as_object() {
        for (a, b) in o {
            out.push((a.clone(), b.as_str().unwrap_or("").to_string()));
        }
    }
    out
}

fn params_of(c: &Value, skip: &[&str]) -> Value {
    match c.as_object() {
        Some(o) => Value::Object(
            o.iter()
                .filter(|(k, _)| !skip.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        None => Value::Null,
    }
}

pub fn view(v: &Value, fallback_name: &str) -> CircuitView {
    let keys = v
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let c = v.get("circuit").unwrap_or(v);
    let kind = c
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("netlist")
        .to_string();
    let mut components = vec![];
    if kind == "modes" {
        for (i, e) in c
            .get("elements")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default()
            .iter()
            .enumerate()
        {
            let modes = e.get("modes").map(|m| m.to_string()).unwrap_or_default();
            components.push(Node {
                id: format!("{i}: modes {modes}"),
                kind: e
                    .get("model")
                    .and_then(|m| m.as_str())
                    .unwrap_or("?")
                    .into(),
                params: params_of(e, &["model", "modes"]),
            });
        }
    } else if kind == "matrix" {
        components.push(Node {
            id: "matrix".into(),
            kind: "transfer matrix".into(),
            params: Value::Null,
        });
    } else {
        let list = c
            .get("instances")
            .or_else(|| c.get("components"))
            .or_else(|| c.get("elements"));
        if let Some(l) = list {
            if let Some(o) = l.as_object() {
                for (k, x) in o {
                    let kind = x
                        .get("model")
                        .or_else(|| x.get("type"))
                        .or_else(|| x.get("component"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("?");
                    components.push(Node {
                        id: k.clone(),
                        kind: kind.into(),
                        params: params_of(x, &["model", "type", "component"]),
                    });
                }
            } else if let Some(a) = l.as_array() {
                for (i, x) in a.iter().enumerate() {
                    let id = x
                        .get("id")
                        .or_else(|| x.get("name"))
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("c{i}"));
                    let kind = x
                        .get("model")
                        .or_else(|| x.get("type"))
                        .or_else(|| x.get("kind"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("?");
                    components.push(Node {
                        id,
                        kind: kind.into(),
                        params: params_of(x, &["id", "name", "model", "type", "kind"]),
                    });
                }
            }
        }
    }
    let tasks = v
        .get("tasks")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .enumerate()
                .map(|(i, t)| Node {
                    id: t
                        .get("id")
                        .and_then(|x| x.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("task{i}")),
                    kind: t.get("type").and_then(|x| x.as_str()).unwrap_or("?").into(),
                    params: params_of(t, &["id", "type"]),
                })
                .collect()
        })
        .unwrap_or_default();
    let sweep = v.get("sweep").map(|s| match s {
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.get("pointer").and_then(|p| p.as_str()))
            .collect::<Vec<_>>()
            .join(" × "),
        _ => s
            .get("pointer")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string(),
    });
    let mut wavelengths: Vec<f64> = v
        .get("wavelengths")
        .and_then(|w| w.as_array())
        .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
        .unwrap_or_default();
    if let Some(w) = v.get("wavelength").and_then(|w| w.as_f64()) {
        wavelengths.push(w);
    }
    CircuitView {
        name: v
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(fallback_name)
            .to_string(),
        recognised: !components.is_empty(),
        kind,
        components,
        connections: pairs(
            c.get("connections")
                .or_else(|| c.get("links"))
                .unwrap_or(&Value::Null),
        ),
        ports: pairs(c.get("ports").unwrap_or(&Value::Null)),
        inputs: strs(c.get("inputs").unwrap_or(&Value::Null)),
        outputs: strs(c.get("outputs").unwrap_or(&Value::Null)),
        tasks,
        sweep,
        wavelengths,
        keys,
    }
}

/// Progress of a run, as the program reports it.
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub fraction: f64,
    pub message: String,
    pub elapsed: f64,
}

/// lightsim-quantum through its command-line program.
pub struct ExternalEngine {
    pub program: PathBuf,
}

impl ExternalEngine {
    /// `GAMMA_LIGHTSIM_QUANTUM`, then `lightsim-quantum` next to `beside` (gamma's own folder), then PATH.
    pub fn find(beside: Option<&Path>) -> Option<ExternalEngine> {
        if let Ok(p) = std::env::var("GAMMA_LIGHTSIM_QUANTUM") {
            let p = PathBuf::from(p);
            return p.is_file().then_some(ExternalEngine { program: p });
        }
        let name = if cfg!(windows) {
            "lightsim-quantum.exe"
        } else {
            "lightsim-quantum"
        };
        if let Some(d) = beside {
            let p = d.join(name);
            if p.is_file() {
                return Some(ExternalEngine { program: p });
            }
        }
        std::env::var_os("PATH")
            .and_then(|p| {
                std::env::split_paths(&p)
                    .map(|d| d.join(name))
                    .find(|f| f.is_file())
            })
            .map(|program| ExternalEngine { program })
    }

    pub fn version(&self) -> Option<String> {
        let out = std::process::Command::new(&self.program)
            .arg("--version")
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Run a document; progress lines go to `on_progress`; returns the result JSON.
    pub fn run(&self, circuit: &Path, on_progress: &mut dyn FnMut(Progress)) -> Result<Value> {
        let mut child = std::process::Command::new(&self.program)
            .arg("run")
            .arg(circuit)
            .arg("--stream")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", self.program.display()))?;
        let stdout = child.stdout.take().unwrap();
        let mut result = None;
        let mut error = None;
        for line in BufReader::new(stdout).lines() {
            let line = line?;
            let Ok(v) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            match v["event"].as_str() {
                Some("progress") => on_progress(Progress {
                    fraction: v["fraction"].as_f64().unwrap_or(0.0),
                    message: v["message"].as_str().unwrap_or("").into(),
                    elapsed: v["elapsed"].as_f64().unwrap_or(0.0),
                }),
                Some("result") => result = Some(v["result"].clone()),
                Some("error") => error = Some(v["message"].as_str().unwrap_or("error").to_string()),
                _ => {}
            }
        }
        let mut err_text = String::new();
        if let Some(mut e) = child.stderr.take() {
            use std::io::Read;
            let _ = e.read_to_string(&mut err_text);
        }
        let st = child.wait()?;
        if let Some(e) = error {
            bail!("{e}");
        }
        match result {
            Some(r) if st.success() => Ok(r),
            _ => bail!(
                "lightsim-quantum exited with {st}: {}",
                err_text.lines().last().unwrap_or("")
            ),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Summary {
    pub plots: Vec<Value>,
    pub bars: Vec<Value>,
    pub tables: Vec<Value>,
}

/// Scalar keys worth plotting per task type.
fn scalar_keys(ty: &str) -> &'static [&'static str] {
    match ty {
        "hom" => &["coincidence", "coincidence_distinguishable", "visibility"],
        "fock" => &["probability_of_success", "fidelity"],
        "gate" => &[
            "success_probability",
            "process_fidelity",
            "average_fidelity",
        ],
        "ring_source" => &["mean_pairs", "purity", "schmidt_number"],
        "transfer" => &["unitarity_error"],
        _ => &[],
    }
}

fn pointer_label(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).replace('_', " ")
}

/// Plots, bar charts and tables for a result document.
pub fn summarize(r: &Value) -> Summary {
    let mut s = Summary::default();
    // a top-level sweep: scalars of each task against the swept value
    if let Some(points) = r.get("sweep").and_then(|x| x.as_array()) {
        let pointer = points
            .first()
            .and_then(|p| p["point"].as_object())
            .and_then(|o| o.keys().next().cloned())
            .unwrap_or_default();
        let x: Vec<f64> = points
            .iter()
            .map(|p| p["point"][&pointer].as_f64().unwrap_or(0.0))
            .collect();
        let ntasks = points
            .first()
            .and_then(|p| p["results"].as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        for t in 0..ntasks {
            let first = &points[0]["results"][t];
            let ty = first["type"].as_str().unwrap_or("");
            let series: Vec<Value> = scalar_keys(ty)
                .iter()
                .filter(|k| first.get(**k).is_some())
                .map(|k| json!({"name": k.replace('_', " "), "y": points.iter().map(|p| p["results"][t][*k].as_f64().unwrap_or(f64::NAN)).collect::<Vec<_>>()}))
                .collect();
            if !series.is_empty() {
                s.plots.push(json!({"title": format!("{} ({ty})", first["id"].as_str().unwrap_or("task")), "x": x, "xlabel": pointer_label(&pointer), "series": series}));
            }
        }
        if let Some(last) = points.first() {
            summarize_results(&last["results"], &mut s, true);
        }
        return s;
    }
    summarize_results(&r["results"], &mut s, false);
    s
}

fn summarize_results(results: &Value, s: &mut Summary, skip_scalars: bool) {
    for t in results.as_array().cloned().unwrap_or_default() {
        let id = t["id"].as_str().unwrap_or("task").to_string();
        // a task over several frequencies
        if let Some(sw) = t.get("sweep").and_then(|x| x.as_array()) {
            let ty = sw
                .first()
                .and_then(|f| f["type"].as_str())
                .unwrap_or("")
                .to_string();
            let x: Vec<f64> = sw
                .iter()
                .map(|f| f["wavelength"].as_f64().unwrap_or(0.0) * 1e9)
                .collect();
            let mut series: Vec<Value> = scalar_keys(&ty).iter().filter(|k| sw[0].get(**k).is_some()).map(|k| json!({"name": k.replace('_', " "), "y": sw.iter().map(|f| f[*k].as_f64().unwrap_or(f64::NAN)).collect::<Vec<_>>()})).collect();
            if let Some(tr) = sw[0]["transmission"].as_array() {
                for m in 0..tr.len() {
                    series.push(json!({"name": format!("transmission {m}"), "y": sw.iter().map(|f| f["transmission"][m].as_f64().unwrap_or(f64::NAN)).collect::<Vec<_>>()}));
                }
            }
            if !series.is_empty() {
                s.plots.push(json!({"title": format!("{id} ({ty})"), "x": x, "xlabel": "wavelength (nm)", "series": series}));
            }
            if let Some(first) = sw.first() {
                summarize_one(&id, first, s, true);
            }
            continue;
        }
        summarize_one(&id, &t, s, skip_scalars);
    }
}

fn summarize_one(id: &str, t: &Value, s: &mut Summary, skip_scalars: bool) {
    let ty = t["type"].as_str().unwrap_or("");
    let wl = t["wavelength"]
        .as_f64()
        .map(|w| format!(" at {:.1} nm", w * 1e9))
        .unwrap_or_default();
    if let Some(out) = t["outcomes"].as_array() {
        let mut v: Vec<(String, f64)> = out
            .iter()
            .map(|o| {
                (
                    o["occupation"]
                        .as_array()
                        .map(|a| a.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(""))
                        .unwrap_or_default(),
                    o["probability"].as_f64().unwrap_or(0.0),
                )
            })
            .collect();
        v.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        v.truncate(24);
        s.bars.push(json!({"title": format!("{id}: output patterns{wl}"), "labels": v.iter().map(|x| &x.0).collect::<Vec<_>>(), "values": v.iter().map(|x| x.1).collect::<Vec<_>>()}));
    }
    for (key, label) in [("threshold", "clicks"), ("pnr", "pattern")] {
        if let Some(a) = t[key].as_array() {
            let v: Vec<(String, f64)> = a
                .iter()
                .take(24)
                .map(|o| {
                    (
                        o[label]
                            .as_array()
                            .map(|x| x.iter().map(|y| y.to_string()).collect::<Vec<_>>().join(""))
                            .unwrap_or_else(|| o[label].to_string()),
                        o["probability"].as_f64().unwrap_or(0.0),
                    )
                })
                .collect();
            s.bars.push(json!({"title": format!("{id}: {key}{wl}"), "labels": v.iter().map(|x| &x.0).collect::<Vec<_>>(), "values": v.iter().map(|x| x.1).collect::<Vec<_>>()}));
        }
    }
    if let (Some(re), Some(im)) = (t["matrix"]["re"].as_array(), t["matrix"]["im"].as_array()) {
        let rows: Vec<Vec<String>> = re
            .iter()
            .zip(im)
            .map(|(a, b)| {
                a.as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .zip(b.as_array().unwrap_or(&vec![]))
                    .map(|(x, y)| {
                        format!(
                            "{:.3}",
                            x.as_f64().unwrap_or(0.0).powi(2) + y.as_f64().unwrap_or(0.0).powi(2)
                        )
                    })
                    .collect()
            })
            .collect();
        s.tables
            .push(json!({"title": format!("{id}: |T|²{wl}"), "rows": rows}));
    }
    if !skip_scalars {
        let keys = scalar_keys(ty);
        let rows: Vec<Vec<String>> = keys
            .iter()
            .filter_map(|k| {
                t[*k]
                    .as_f64()
                    .map(|v| vec![k.replace('_', " "), format!("{v:.4}")])
            })
            .collect();
        if !rows.is_empty() {
            s.tables
                .push(json!({"title": format!("{id} ({ty}){wl}"), "rows": rows}));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn netlist_document() {
        let v: Value = serde_json::from_str(
            r#"{"format":"lightsim-quantum/1","name":"Lossy MZI","circuit":{"kind":"netlist","instances":{"dc1":{"model":"beam_splitter","cross":0.5},"ps":{"model":"phase_shifter","phase":1.2}},
            "connections":[["dc1.out0","ps.in"]],"ports":[["in0","dc1.in0"]],"inputs":["in0"],"outputs":["out0"]},"wavelengths":[1.55e-6],
            "tasks":[{"id":"two_photons","type":"distribution","input":[1,1]}]}"#,
        )
        .unwrap();
        let c = view(&v, "x");
        assert!(c.recognised);
        assert_eq!(c.name, "Lossy MZI");
        assert_eq!(c.components.len(), 2);
        assert_eq!(c.components[0].kind, "beam_splitter");
        assert_eq!(c.connections[0], ("dc1.out0".into(), "ps.in".into()));
        assert_eq!(c.tasks[0].kind, "distribution");
        let modes: Value = serde_json::from_str(r#"{"circuit":{"kind":"modes","modes":2,"elements":[{"modes":[0,1],"model":"beam_splitter"}]}}"#).unwrap();
        assert_eq!(view(&modes, "m").components[0].kind, "beam_splitter");
    }

    #[test]
    fn summaries() {
        let hom: Value = serde_json::from_str(
            r#"{"format":"lightsim-quantum/1","sweep":[{"point":{"/tasks/0/photons/1/delay":-1e-12},"results":[{"id":"hom","type":"hom","coincidence":0.4,"coincidence_distinguishable":0.5,"visibility":0.2}]},
            {"point":{"/tasks/0/photons/1/delay":0},"results":[{"id":"hom","type":"hom","coincidence":0.01,"coincidence_distinguishable":0.5,"visibility":0.98}]}]}"#,
        )
        .unwrap();
        let s = summarize(&hom);
        assert_eq!(s.plots.len(), 1);
        assert_eq!(s.plots[0]["xlabel"], "delay");
        assert_eq!(s.plots[0]["series"][0]["y"][1], 0.01);
        let dist: Value = serde_json::from_str(r#"{"results":[{"id":"d","type":"distribution","wavelength":1.55e-6,"outcomes":[{"occupation":[1,1],"probability":0.1},{"occupation":[2,0],"probability":0.45}]},
            {"id":"T","sweep":[{"type":"transfer","wavelength":1.54e-6,"transmission":[0.9,0.9],"unitarity_error":0.1,"matrix":{"re":[[1,0],[0,1]],"im":[[0,0],[0,0]]}},{"type":"transfer","wavelength":1.55e-6,"transmission":[0.8,0.8],"unitarity_error":0.2,"matrix":{"re":[[1,0],[0,1]],"im":[[0,0],[0,0]]}}]}]}"#).unwrap();
        let s = summarize(&dist);
        assert_eq!(s.bars[0]["labels"][0], "20");
        assert_eq!(s.plots[0]["series"].as_array().unwrap().len(), 3);
        assert_eq!(s.tables[0]["rows"][0][0], "1.000");
    }
}
