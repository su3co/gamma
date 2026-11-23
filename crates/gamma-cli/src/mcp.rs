//! `gamma mcp`: gamma's tools for agents, as a Model Context Protocol server
//! on standard input and output (JSON-RPC 2.0, one message per line). Every
//! tool forwards to the hub, so the person sees the agent's actions live.

use crate::client::{Client, enc};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::PathBuf;

const PROTOCOL: &str = "2025-06-18";

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: Value,
}

fn obj(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn tools() -> Vec<Tool> {
    let s = |d: &str| json!({"type": "string", "description": d});
    let n = |d: &str| json!({"type": "number", "description": d});
    let b = |d: &str| json!({"type": "boolean", "description": d});
    vec![
        Tool {
            name: "project_info",
            description: "Overview of the open gamma project: manifest, layout summary (cells, layers, ports, size), simulations, latest runs, circuits, checkpoints and what the person has selected.",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "layout_summary",
            description: "Cells, layers (with stack names, materials, z range, polygon counts), ports (position, width, orientation) and bounding box of the layout. Units: um.",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "layout_query",
            description: "Polygons of one layer inside a box (um): references, owning cell, bounding boxes, vertex counts, areas.",
            schema: obj(
                json!({"layer": s("GDS layer/datatype, e.g. 1/0"), "x0": n("um"), "y0": n("um"), "x1": n("um"), "y1": n("um"), "limit": n("max polygons (default 200)")}),
                &["layer"],
            ),
        },
        Tool {
            name: "layout_pick",
            description: "What is at a point of the layout (um): a port, else the top-most polygon, with a description.",
            schema: obj(
                json!({"x": n("um"), "y": n("um"), "tol": n("tolerance um (default 0.05)")}),
                &["x", "y"],
            ),
        },
        Tool {
            name: "layout_measure",
            description: "Edge-to-edge distance between two polygons given as references (poly:1/0#3), e.g. a coupling gap.",
            schema: obj(
                json!({"a": s("poly:L/D#i"), "b": s("poly:L/D#i")}),
                &["a", "b"],
            ),
        },
        Tool {
            name: "stack_get",
            description: "The layer stack: layers (GDS layer, z range, material, colour, design rules) and materials (index, lightsim model).",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "stack_set",
            description: "Replace the layer stack (same structure as stack_get returns). Prefer editing stack.toml for small changes.",
            schema: obj(json!({"stack": json!({"type": "object"})}), &["stack"]),
        },
        Tool {
            name: "place_component",
            description: "Place (or replace) a gdsfactory component instance in a YAML netlist (layout/*.pic.yml), then rebuild. Example: component mmi1x2, settings {length_mmi: 5.5}.",
            schema: obj(
                json!({"netlist": s("netlist file, default layout/top.pic.yml"), "name": s("instance name"), "component": s("gdsfactory cell name"), "settings": json!({"type": "object"}), "x": n("um"), "y": n("um"), "rotation": n("degrees"), "mirror": b("mirror")}),
                &["name", "component"],
            ),
        },
        Tool {
            name: "route",
            description: "Route between two instance ports in a YAML netlist with a gdsfactory router (routes section), then rebuild. Ports as instance,port (e.g. mmi,o2).",
            schema: obj(
                json!({"netlist": s("netlist file, default layout/top.pic.yml"), "from": s("instance,port"), "to": s("instance,port"), "bundle": s("route bundle name (default r_<from>)"), "cross_section": s("e.g. strip")}),
                &["from", "to"],
            ),
        },
        Tool {
            name: "remove_component",
            description: "Remove an instance (and its connections) from a YAML netlist, then rebuild.",
            schema: obj(
                json!({"netlist": s("netlist file"), "name": s("instance name")}),
                &["name"],
            ),
        },
        Tool {
            name: "expose_port",
            description: "Make an instance port a port of the top cell in a YAML netlist.",
            schema: obj(
                json!({"netlist": s("netlist file"), "port": s("top-level port name"), "instance_port": s("instance,port")}),
                &["port", "instance_port"],
            ),
        },
        Tool {
            name: "build",
            description: "Regenerate files from their sources: the GDS and ports from the layout generator, and simulation specs from their generators. target: all, layout or sim:<name>. Returns the scripts' output.",
            schema: obj(json!({"target": s("all | layout | sim:<name>")}), &[]),
        },
        Tool {
            name: "sim_list",
            description: "Simulations of the project (name, spec file, generator, built or not).",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "sim_setup",
            description: "What a simulation describes as lightsim builds it: grid (cells, steps, time step, memory), media, structures, sources, monitors, boundaries, wavelength range, and run-time estimates per device. preview: the coarse variant.",
            schema: obj(
                json!({"sim": s("simulation name"), "preview": b("coarse preview variant")}),
                &["sim"],
            ),
        },
        Tool {
            name: "sim_run",
            description: "Start a simulation on a device: cpu, cuda:0, cuda:0,1 (this machine) or on a remote host (host + device). Returns the job; wait_s waits up to that many seconds for it to finish.",
            schema: obj(
                json!({"sim": s("simulation name"), "device": s("cpu | cuda:0 | cuda:0,1"), "host": s("remote host name"), "preview": b("run the coarse preview"), "wait_s": n("seconds to wait (0 = return at once)")}),
                &["sim"],
            ),
        },
        Tool {
            name: "job_status",
            description: "Status, plan, progress (step, energy decay, throughput) and log of a job; without job: all jobs.",
            schema: obj(json!({"job": s("job id")}), &[]),
        },
        Tool {
            name: "job_cancel",
            description: "Cancel a queued or running job (a running job keeps the results so far).",
            schema: obj(json!({"job": s("job id")}), &["job"]),
        },
        Tool {
            name: "results",
            description: "Results of a run (latest finished run by default): overview (stats, monitors, S-parameters, metrics), or a monitor's spectrum, time series, energy decay or field-map statistics.",
            schema: obj(
                json!({"sim": s("simulation"), "run": s("run id"), "what": s("overview | spectrum | sparams | time | decay | field"), "monitor": s("monitor name"), "quantity": s("for field: |E|2, Ey, abs(Ey), phase(Ey)"), "f": n("frequency index")}),
                &["sim"],
            ),
        },
        Tool {
            name: "check",
            description: "Design-rule check (min width and spacing per layer from the stack) and consistency checks (stale or missing generated files, unmapped layers, mesh resolution, absorbing layers, sources and monitors outside the region, spec vs layout).",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "view_render",
            description: "Render a viewer to an image to look at it: layout (highlight=refs separated by ;, window=x0,y0,x1,y1), 3d, setup (sim, axis, coord, preview), field (sim, monitor, quantity, f, run), live (job), spectrum, sparams, decay, time.",
            schema: obj(
                json!({"view": s("layout | 3d | setup | field | live | spectrum | sparams | decay | time"), "sim": s(""), "run": s(""), "monitor": s(""), "quantity": s(""), "f": n(""), "axis": n("0, 1, 2"), "coord": n("m"), "preview": b(""), "job": s(""), "highlight": s(""), "window": s(""), "theme": s("dark | light"), "width": n("px"), "height": n("px")}),
                &["view"],
            ),
        },
        Tool {
            name: "selection_get",
            description: "The object the person selected in a viewer (reference and description), if any.",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "checkpoint_create",
            description: "Commit the project and tag it as a checkpoint with a message (gamma also checkpoints at the end of every agent turn).",
            schema: obj(json!({"message": s("what changed")}), &[]),
        },
        Tool {
            name: "checkpoint_list",
            description: "Checkpoints (number, commit, time, actor, message) and review comments.",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "checkpoint_diff",
            description: "Compare checkpoint a with b (or the current state): files changed, layout diff (polygons added/removed per layer, ports moved) and result diff (metrics of the latest runs).",
            schema: obj(
                json!({"a": n("checkpoint"), "b": n("checkpoint (default: current state)")}),
                &["a"],
            ),
        },
        Tool {
            name: "comment_add",
            description: "Add a review comment, optionally anchored to an object reference and a checkpoint.",
            schema: obj(
                json!({"text": s(""), "object": s("e.g. port:o1"), "checkpoint": n("")}),
                &["text"],
            ),
        },
        Tool {
            name: "remotes_list",
            description: "Remote simulation hosts known to gamma.",
            schema: obj(json!({}), &[]),
        },
        Tool {
            name: "circuit_list",
            description: "Quantum circuits of the project (lightsim-quantum) and whether the engine is installed.",
            schema: obj(json!({}), &[]),
        },
    ]
}

fn text(v: impl Into<String>) -> Value {
    json!({"content": [{"type": "text", "text": v.into()}]})
}

fn jtext(v: &Value) -> Value {
    text(serde_json::to_string_pretty(v).unwrap_or_default())
}

struct Server {
    client: Client,
    project: String,
    root: PathBuf,
}

fn q(args: &Value, keys: &[&str]) -> String {
    let mut parts = vec![];
    for k in keys {
        if let Some(v) = args.get(*k) {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Bool(b) => {
                    if *b {
                        "1".into()
                    } else {
                        "0".into()
                    }
                }
                Value::Null => continue,
                other => other.to_string(),
            };
            parts.push(format!("{k}={}", enc(&s)));
        }
    }
    parts.join("&")
}

impl Server {
    fn p(&self, rest: &str) -> String {
        format!("/api/p/{}{rest}", self.project)
    }

    fn latest_run(&self, sim: &str) -> Result<String> {
        let runs = self
            .client
            .get(&self.p(&format!("/runs?sim={}", enc(sim))))?;
        runs.as_array()
            .and_then(|a| a.iter().find(|r| r["has_results"] == true))
            .and_then(|r| r["id"].as_str().map(|s| s.to_string()))
            .with_context(|| format!("simulation {sim} has no finished run yet"))
    }

    fn netlist_edit(
        &self,
        args: &Value,
        f: impl FnOnce(&mut gamma_core::netlist::Netlist) -> Result<String>,
    ) -> Result<Value> {
        let rel = args["netlist"].as_str().unwrap_or("layout/top.pic.yml");
        if rel.contains("..") || rel.starts_with('/') {
            bail!("netlist path must be inside the project");
        }
        let path = self.root.join(rel);
        let mut n = if path.is_file() {
            gamma_core::netlist::Netlist::load(&path)?
        } else {
            gamma_core::netlist::Netlist::new("top")
        };
        let msg = f(&mut n)?;
        n.save(&path)?;
        let b = self
            .client
            .post(&self.p("/build"), json!({"target": "layout"}));
        Ok(match b {
            Ok(_) => text(format!("{msg}; layout rebuilt from {rel}")),
            Err(e) => text(format!("{msg}; saved {rel}, but the build failed: {e:#}")),
        })
    }

    fn call(&self, name: &str, args: &Value) -> Result<Value> {
        let c = &self.client;
        Ok(match name {
            "project_info" => {
                let info = c.get(&self.p(""))?;
                let layout = c.get(&self.p("/layout"))?;
                let sims = c.get(&self.p("/sims"))?;
                let runs = c.get(&self.p("/runs"))?;
                let sel = c.get(&self.p("/selection"))?;
                let l = &layout["layout"];
                let short = if l.is_null() {
                    json!(null)
                } else {
                    json!({"top": l["top"], "bbox": l["bbox"], "polygons": l["polygons"], "layers": l["layers"].as_array().map(|a| a.iter().map(|x| json!({"layer": x["reference"], "name": x["name"], "polygons": x["polygons"]})).collect::<Vec<_>>()), "ports": l["ports"]})
                };
                let recent: Vec<Value> = runs.as_array().map(|a| a.iter().take(8).map(|r| json!({"sim": r["sim"], "id": r["id"], "status": r["run"]["status"], "device": r["run"]["device"], "host": r["run"]["host"]})).collect()).unwrap_or_default();
                jtext(&json!({"project": {"name": info["name"], "root": info["root"], "checkpoints": info["checkpoints"], "layout_error": info["layout_error"]}, "layout": short, "simulations": sims, "recent_runs": recent, "selection": sel, "manifest": info["manifest"]}))
            }
            "layout_summary" => jtext(&c.get(&self.p("/layout"))?["layout"]),
            "layout_query" => jtext(&c.get(&self.p(&format!("/layout/query?{}", q(args, &["layer", "x0", "y0", "x1", "y1", "limit"]))))?),
            "layout_pick" => jtext(&c.get(&self.p(&format!("/layout/pick?{}", q(args, &["x", "y", "tol"]))))?),
            "layout_measure" => jtext(&c.get(&self.p(&format!("/layout/gap?{}", q(args, &["a", "b"]))))?),
            "stack_get" => jtext(&c.get(&self.p("/stack"))?),
            "stack_set" => jtext(&c.put(&self.p("/stack"), args["stack"].clone())?),
            "place_component" => self.netlist_edit(args, |n| {
                let name = args["name"].as_str().context("name required")?;
                let comp = args["component"].as_str().context("component required")?;
                n.place(name, comp, args.get("settings").cloned(), args["x"].as_f64().unwrap_or(0.0), args["y"].as_f64().unwrap_or(0.0), args["rotation"].as_f64().unwrap_or(0.0), args["mirror"].as_bool().unwrap_or(false))?;
                Ok(format!("placed {name} ({comp})"))
            })?,
            "route" => self.netlist_edit(args, |n| {
                let a = args["from"].as_str().context("from required")?;
                let b = args["to"].as_str().context("to required")?;
                let bundle = args["bundle"].as_str().map(|s| s.to_string()).unwrap_or_else(|| format!("r_{}", a.replace(',', "_")));
                n.route(&bundle, a, b, args["cross_section"].as_str())?;
                Ok(format!("routed {a} -> {b}"))
            })?,
            "remove_component" => self.netlist_edit(args, |n| {
                let name = args["name"].as_str().context("name required")?;
                if !n.remove(name) {
                    bail!("no instance {name}");
                }
                Ok(format!("removed {name}"))
            })?,
            "expose_port" => self.netlist_edit(args, |n| {
                let p = args["port"].as_str().context("port required")?;
                let ip = args["instance_port"].as_str().context("instance_port required")?;
                n.expose(p, ip);
                Ok(format!("exposed {ip} as {p}"))
            })?,
            "build" => jtext(&c.post(&self.p("/build"), json!({"target": args["target"].as_str().unwrap_or("all")}))?),
            "sim_list" => jtext(&c.get(&self.p("/sims"))?),
            "sim_setup" => {
                let sim = args["sim"].as_str().context("sim required")?;
                let mut v = c.get(&self.p(&format!("/sims/{}/setup?{}", enc(sim), q(args, &["preview"]))))?;
                // structures can be many: keep the first ones
                if let Some(o) = v["objects"].as_array_mut() {
                    let n = o.len();
                    let mut kept: Vec<Value> = o.iter().filter(|x| x["kind"] != "structure").cloned().collect();
                    let st: Vec<Value> = o.iter().filter(|x| x["kind"] == "structure").take(20).cloned().collect();
                    kept.extend(st);
                    *o = kept;
                    v["objects_total"] = json!(n);
                }
                jtext(&v)
            }
            "sim_run" => {
                let sim = args["sim"].as_str().context("sim required")?;
                let body = json!({"device": args["device"], "host": args["host"], "preview": args["preview"].as_bool().unwrap_or(false)});
                let job = c.post(&self.p(&format!("/sims/{}/run", enc(sim))), body)?;
                let wait = args["wait_s"].as_f64().unwrap_or(0.0);
                if wait > 0.0 {
                    let id = job["id"].as_str().unwrap_or("").to_string();
                    let j = c.wait_job(&id, Some(std::time::Duration::from_secs_f64(wait)), |_| {})?;
                    jtext(&j)
                } else {
                    jtext(&job)
                }
            }
            "job_status" => match args["job"].as_str() {
                Some(j) => jtext(&c.get(&format!("/api/jobs/{}", enc(j)))?),
                None => jtext(&c.get("/api/jobs")?),
            },
            "job_cancel" => jtext(&c.post(&format!("/api/jobs/{}/cancel", enc(args["job"].as_str().context("job required")?)), json!({}))?),
            "results" => {
                let sim = args["sim"].as_str().context("sim required")?;
                let run = match args["run"].as_str() {
                    Some(r) => r.to_string(),
                    None => self.latest_run(sim)?,
                };
                let what = args["what"].as_str().unwrap_or("overview");
                let mut v = c.get(&self.p(&format!("/runs/{}/{}/{}?{}", enc(sim), enc(&run), what, q(args, &["monitor", "f"]) + &args["quantity"].as_str().map(|s| format!("&q={}", enc(s))).unwrap_or_default())))?;
                if what == "field" {
                    // statistics instead of the samples; view_render shows the map
                    let dims = v["dims"].clone();
                    v["data"] = json!(format!("{} x {} samples (use view_render view=field to see it)", dims[0], dims[1]));
                    v["u"] = json!({"from": v["u"][0], "count": dims[0]});
                    v["v"] = json!({"from": v["v"][0], "count": dims[1]});
                }
                v["run"] = json!(run);
                jtext(&v)
            }
            "export_sparams" => {
                let sim = args["sim"].as_str().context("sim required")?;
                let run = match args["run"].as_str() {
                    Some(r) => r.to_string(),
                    None => self.latest_run(sim)?,
                };
                let v = c.post(&self.p(&format!("/runs/{}/{}/export", enc(sim), enc(&run))), json!({"format": args["format"].as_str().unwrap_or("touchstone")}))?;
                text(format!("wrote {}", v["path"].as_str().unwrap_or("")))
            }
            "check" => jtext(&c.get(&self.p("/check"))?),
            "view_render" => {
                let view = args["view"].as_str().unwrap_or("layout");
                let png = c.get_bytes(&self.p(&format!("/render?view={}&{}", enc(view), q(args, &["sim", "run", "monitor", "quantity", "f", "axis", "coord", "preview", "job", "highlight", "window", "theme", "width", "height"]))))?;
                use base64::Engine;
                json!({"content": [{"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png"}]})
            }
            "selection_get" => jtext(&c.get(&self.p("/selection"))?),
            "checkpoint_create" => jtext(&c.post(&self.p("/checkpoints"), json!({"message": args["message"].as_str().unwrap_or("Checkpoint")}))?),
            "checkpoint_list" => jtext(&c.get(&self.p("/checkpoints"))?),
            "checkpoint_diff" => jtext(&c.get(&self.p(&format!("/checkpoints/diff?{}", q(args, &["a", "b"]))))?),
            "comment_add" => jtext(&c.post(&self.p("/comments"), json!({"text": args["text"], "object": args["object"], "checkpoint": args["checkpoint"]}))?),
            "remotes_list" => jtext(&c.get("/api/remotes")?),
            "circuit_list" => jtext(&c.get(&self.p("/circuits"))?),
            "circuit_view" => jtext(&c.get(&self.p(&format!("/circuits/{}", enc(args["name"].as_str().context("name required")?))))?),
            "circuit_run" => jtext(&c.post(&self.p(&format!("/circuits/{}/run", enc(args["name"].as_str().context("name required")?))), json!({}))?),
            "circuit_results" => jtext(&c.get(&self.p(&format!("/circuits/{}/results", enc(args["name"].as_str().context("name required")?))))?),
            other => bail!("unknown tool {other}"),
        })
    }
}

pub fn serve(project: Option<PathBuf>) -> Result<()> {
    let session = std::env::var("GAMMA_AGENT_SESSION")
        .unwrap_or_else(|_| format!("mcp-{:04x}", rand::random::<u16>()));
    let client = Client::connect(&format!("agent:{session}"))?;
    let dir = project.unwrap_or(std::env::current_dir()?);
    let root = gamma_core::Project::discover(&dir)?.root;
    let id = client.open(&root)?;
    let server = Server {
        client,
        project: id,
        root,
    };
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            writeln!(
                out,
                "{}",
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": "parse error"}})
            )?;
            out.flush()?;
            continue;
        };
        let Some(id) = msg.get("id").cloned() else {
            continue;
        }; // notification
        let method = msg["method"].as_str().unwrap_or("");
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "gamma", "version": gamma_core::VERSION},
                "instructions": "gamma: tools for designing, simulating and testing photonic integrated circuits in the open project. The person watches the viewers update as you work."
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(
                json!({"tools": tools().iter().map(|t| json!({"name": t.name, "description": t.description, "inputSchema": t.schema})).collect::<Vec<_>>()}),
            ),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let args = msg["params"].get("arguments").cloned().unwrap_or(json!({}));
                match server.call(name, &args) {
                    Ok(v) => Ok(v),
                    Err(e) => Ok(
                        json!({"content": [{"type": "text", "text": format!("error: {e:#}")}], "isError": true}),
                    ),
                }
            }
            "resources/list" => Ok(json!({"resources": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            other => Err((-32601, format!("method not found: {other}"))),
        };
        let reply = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, m)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}})
            }
        };
        writeln!(out, "{reply}")?;
        out.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tool_schemas_are_objects() {
        let t = tools();
        assert!(t.len() > 20);
        let mut names: Vec<&str> = t.iter().map(|x| x.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), t.len());
        for x in &t {
            assert_eq!(x.schema["type"], "object", "{}", x.name);
        }
    }
}
