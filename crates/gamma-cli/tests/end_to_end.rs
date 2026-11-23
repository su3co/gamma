//! End to end: a hub process, a project with a GDS and a lightsim spec, the
//! HTTP API, a CPU run through a worker process, checkpoints, and the MCP
//! server as an agent would use it.

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const GAMMA: &str = env!("CARGO_BIN_EXE_gamma");

struct Hub {
    child: Child,
    url: String,
    token: String,
    env: Vec<(String, PathBuf)>,
}

impl Drop for Hub {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[allow(clippy::zombie_processes)] // waited for in Drop
fn start_hub(dir: &Path) -> Hub {
    let env = vec![
        ("GAMMA_RUNTIME_DIR".to_string(), dir.join("rt")),
        ("GAMMA_DATA_DIR".to_string(), dir.join("data")),
        ("GAMMA_CONFIG_DIR".to_string(), dir.join("cfg")),
    ];
    let mut cmd = Command::new(GAMMA);
    cmd.args(["hub", "--port", "0"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in &env {
        cmd.env(k, v);
    }
    let child = cmd.spawn().unwrap();
    let t0 = Instant::now();
    loop {
        if let Ok(t) = std::fs::read_to_string(dir.join("rt/hub.json")) {
            let v: Value = serde_json::from_str(&t).unwrap();
            return Hub {
                child,
                url: format!("http://127.0.0.1:{}", v["port"]),
                token: v["token"].as_str().unwrap().into(),
                env,
            };
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "hub did not start");
        std::thread::sleep(Duration::from_millis(50));
    }
}

impl Hub {
    fn get(&self, p: &str) -> Value {
        let r = reqwest::blocking::Client::new()
            .get(format!("{}{p}", self.url))
            .bearer_auth(&self.token)
            .send()
            .unwrap();
        let s = r.status();
        let v: Value = r.json().unwrap_or(Value::Null);
        assert!(s.is_success(), "GET {p}: {s} {v}");
        v
    }
    fn bytes(&self, p: &str) -> Vec<u8> {
        let r = reqwest::blocking::Client::new()
            .get(format!("{}{p}", self.url))
            .bearer_auth(&self.token)
            .send()
            .unwrap();
        assert!(r.status().is_success(), "GET {p}: {}", r.status());
        r.bytes().unwrap().to_vec()
    }
    fn post(&self, p: &str, body: Value) -> Value {
        let r = reqwest::blocking::Client::new()
            .post(format!("{}{p}", self.url))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .unwrap();
        let s = r.status();
        let v: Value = r.json().unwrap_or(Value::Null);
        assert!(s.is_success(), "POST {p}: {s} {v}");
        v
    }
}

const SPEC: &str = r#"{
  "name": "wire",
  "region": {"min": [0, -1e-6, -0.6e-6], "max": [3e-6, 1e-6, 0.8e-6]},
  "mesh": {"type": "uniform", "dx": [60e-9, 60e-9, 50e-9]},
  "boundaries": {"x": [{"type": "pml", "layers": 8, "params": {"order": 3, "sigma_scale": 1, "kappa_max": 2, "alpha_max": 0, "stabilize": true}}, {"type": "pml", "layers": 8}],
                 "y": [{"type": "pml", "layers": 8}, {"type": "pml", "layers": 8}],
                 "z": [{"type": "pml", "layers": 8}, {"type": "pml", "layers": 8}]},
  "run_time": 20e-15,
  "background": "sio2",
  "media": [{"name": "sio2", "eps": [2.085, 2.085, 2.085]}, {"name": "si", "eps": [12.08, 12.08, 12.08]}],
  "structures": [{"name": "wire", "medium": "si", "shape": {"type": "poly_slab", "vertices": [[-1e-6, -0.225e-6], [4e-6, -0.225e-6], [4e-6, 0.225e-6], [-1e-6, 0.225e-6]], "slab_min": 0, "slab_max": 0.22e-6}}],
  "sources": [{"type": "point_dipole", "name": "dipole", "position": [0.6e-6, 0, 0.11e-6], "polarization": "Ey",
               "signal": {"type": "gaussian_pulse", "freq0": 1.934e14, "fwidth": 4e13}}],
  "monitors": [
    {"type": "flux", "name": "through", "axis": 0, "position": 2.4e-6, "min": [2.4e-6, -0.9e-6, -0.5e-6], "max": [2.4e-6, 0.9e-6, 0.7e-6], "freqs": [1.85e14, 1.934e14, 2.0e14]},
    {"type": "dft_field", "name": "top", "min": [0, -1e-6, 0.11e-6], "max": [3e-6, 1e-6, 0.11e-6], "components": ["Ey"], "freqs": [1.934e14]}
  ],
  "shutoff": 1e-4
}"#;

fn make_project(dir: &Path) -> PathBuf {
    let root = dir.join("proj");
    std::fs::create_dir_all(root.join("build/sims")).unwrap();
    std::fs::write(
        root.join("gamma.toml"),
        "[project]\nname = \"Wire\"\n\n[layout]\ngds = \"build/wire.gds\"\nports = \"build/wire.ports.json\"\n\n[[simulation]]\nname = \"wire\"\nspec = \"build/sims/wire.json\"\n",
    )
    .unwrap();
    std::fs::write(
        root.join("stack.toml"),
        gamma_core::Stack::soi220().to_toml(),
    )
    .unwrap();
    std::fs::write(root.join(".gitignore"), "build/\n.gamma/local/\n").unwrap();
    let mut w = gamma_core::gds::Writer::new("wire", 1e-9);
    w.begin_cell("wire");
    w.polygon(
        (1, 0),
        &[[-1.0, -0.225], [4.0, -0.225], [4.0, 0.225], [-1.0, 0.225]],
    );
    w.end_cell();
    std::fs::write(root.join("build/wire.gds"), w.finish()).unwrap();
    std::fs::write(root.join("build/wire.ports.json"), r#"[{"name":"o1","x":-1,"y":0,"width":0.45,"orientation":180,"layer":[1,0]},{"name":"o2","x":4,"y":0,"width":0.45,"orientation":0,"layer":[1,0]}]"#).unwrap();
    std::fs::write(root.join("build/sims/wire.json"), SPEC).unwrap();
    root
}

#[test]
fn hub_api_run_checkpoint_and_mcp() {
    let d = tempfile::tempdir().unwrap();
    let root = make_project(d.path());
    let hub = start_hub(d.path());
    // the token is required
    let r = reqwest::blocking::get(format!("{}/api/hub", hub.url)).unwrap();
    assert_eq!(r.status().as_u16(), 401);
    let p = hub.post("/api/open", json!({"path": root}));
    let id = p["id"].as_str().unwrap().to_string();
    let pp = |s: &str| format!("/api/p/{id}{s}");

    let l = hub.get(&pp("/layout"));
    assert_eq!(l["layout"]["ports"].as_array().unwrap().len(), 2);
    assert_eq!(l["layout"]["layers"][0]["name"], "core");
    let mesh = hub.bytes(&pp("/layout/mesh?layer=1/0"));
    assert!(mesh.len() > 8);
    let pick = hub.get(&pp("/layout/pick?x=1&y=0"));
    assert_eq!(pick["reference"], "poly:1/0#0");

    let s = hub.get(&pp("/sims/wire/setup"));
    assert!(s["grid"]["cells"].as_u64().unwrap() > 10_000);
    let sl = hub.get(&pp("/sims/wire/slice?axis=2"));
    assert_eq!(sl["axis"], 2);

    let png = hub.bytes(&pp("/render?view=layout&width=400&height=300"));
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    let png = hub.bytes(&pp("/render?view=setup&sim=wire&width=400&height=300"));
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));

    let checks = hub.get(&pp("/check"));
    assert_eq!(checks["errors"], 0, "{checks}");

    // a CPU run through a worker process
    let job = hub.post(&pp("/sims/wire/run"), json!({"device": "cpu"}));
    let jid = job["id"].as_str().unwrap().to_string();
    let t0 = Instant::now();
    let j = loop {
        let j = hub.get(&format!("/api/jobs/{jid}"));
        if ["done", "failed", "cancelled"].contains(&j["status"].as_str().unwrap()) {
            break j;
        }
        assert!(
            t0.elapsed() < Duration::from_secs(300),
            "run did not finish"
        );
        std::thread::sleep(Duration::from_millis(300));
    };
    assert_eq!(j["status"], "done", "{j}");
    let runs = hub.get(&pp("/runs?sim=wire"));
    let run = runs[0]["id"].as_str().unwrap().to_string();
    // keys this lightsim does not know reach the worker unchanged
    let copied =
        std::fs::read_to_string(root.join("results/wire").join(&run).join("spec.json")).unwrap();
    assert!(
        copied.contains("\"stabilize\": true"),
        "the spec copy dropped an unknown key"
    );
    let o = hub.get(&pp(&format!("/runs/wire/{run}/overview")));
    assert_eq!(o["monitors"].as_array().unwrap().len(), 2);
    let f = hub.get(&pp(&format!("/runs/wire/{run}/field?monitor=top&q=|E|2")));
    assert_eq!(f["axis"], 2);

    // checkpoints
    let c = hub.post(&pp("/checkpoints"), json!({"message": "first"}));
    assert_eq!(c["n"], 1);
    std::fs::write(root.join("notes.md"), "changed").unwrap();
    std::thread::sleep(Duration::from_millis(600));
    let c2 = hub.post(&pp("/checkpoints"), json!({"message": "second"}));
    assert_eq!(c2["n"], 2);
    let diff = hub.get(&pp("/checkpoints/diff?a=1&b=2"));
    assert!(
        diff["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f.as_str().unwrap().contains("notes.md")),
        "{diff}"
    );

    // the event log saw it all
    let ev = hub.get(&pp("/events?limit=500"));
    let kinds: Vec<&str> = ev
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    for k in ["project.opened", "sim.queued", "sim.done", "checkpoint"] {
        assert!(kinds.contains(&k), "missing {k} in {kinds:?}");
    }

    // MCP, as an agent CLI would start it
    let mut cmd = Command::new(GAMMA);
    cmd.args(["mcp", "--project"])
        .arg(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for (k, v) in &hub.env {
        cmd.env(k, v);
    }
    let mut mcp = cmd.spawn().unwrap();
    let mut stdin = mcp.stdin.take().unwrap();
    let mut out = BufReader::new(mcp.stdout.take().unwrap());
    let mut call = |id: u64, method: &str, params: Value| -> Value {
        writeln!(
            stdin,
            "{}",
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params})
        )
        .unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        out.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    };
    let init = call(
        1,
        "initialize",
        json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "gamma");
    let tools = call(2, "tools/list", json!({}));
    assert!(tools["result"]["tools"].as_array().unwrap().len() > 20);
    let info = call(
        3,
        "tools/call",
        json!({"name": "project_info", "arguments": {}}),
    );
    let text = info["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"Wire\""), "{text}");
    let img = call(
        4,
        "tools/call",
        json!({"name": "view_render", "arguments": {"view": "layout", "width": 300, "height": 200}}),
    );
    assert_eq!(img["result"]["content"][0]["type"], "image");
    let res = call(
        5,
        "tools/call",
        json!({"name": "results", "arguments": {"sim": "wire"}}),
    );
    assert!(
        res["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("through")
    );
    let bad = call(
        6,
        "tools/call",
        json!({"name": "results", "arguments": {"sim": "nope"}}),
    );
    assert_eq!(bad["result"]["isError"], true);
    drop(stdin);
    let _ = mcp.wait();
}
