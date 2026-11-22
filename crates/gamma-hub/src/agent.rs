//! Agent sessions inside a project: the person's agent of choice (Claude
//! Code or Codex) runs as a child process of the hub with the project folder
//! as its working directory and gamma's MCP server attached. Its messages and
//! tool calls are events every client shows; the end of each turn is a
//! checkpoint.

use crate::projects::ProjectState;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};

pub const SYSTEM_PROMPT: &str = "You are working inside gamma, a workspace for designing, simulating and testing photonic integrated circuits. \
The person watches the viewers (layout, 3-D, set-up, run, results) update as you work. Use the gamma MCP tools to inspect the layout and stack, \
build generated files, set up and run lightsim simulations, read results, run checks, and render any viewer as an image to look at what you made. \
Edit the project's text files (gdsfactory Python or YAML, stack.toml, simulation generators, gamma.toml) directly; the hub rebuilds and the viewers follow. \
References such as port:o1, poly:1/0#3, monitor:sim/name or point:x,y name objects the person selected; use selection_get for details. \
Prefer a quick preview run (preview: true) or the CPU for small checks and a GPU or remote host for full runs, and state estimated run times before long runs.";

/// The skills gamma ships to agents (installed into the project's `.claude/skills/`).
pub const SKILLS: [(&str, &str); 4] = [
    ("gamma", include_str!("../../../skills/gamma/SKILL.md")),
    (
        "gdsfactory-pic",
        include_str!("../../../skills/gdsfactory-pic/SKILL.md"),
    ),
    (
        "lightsim-fdtd",
        include_str!("../../../skills/lightsim-fdtd/SKILL.md"),
    ),
    (
        "lightsim-quantum",
        include_str!("../../../skills/lightsim-quantum/SKILL.md"),
    ),
];

pub fn install_skills(root: &Path) -> Result<()> {
    for (name, text) in SKILLS {
        gamma_core::fsutil::atomic_write(
            &root.join(".claude/skills").join(name).join("SKILL.md"),
            text.as_bytes(),
        )?;
    }
    Ok(())
}

pub struct Session {
    pub provider: String,
    pub id: String,
    pub stdin: Option<ChildStdin>,
    pub child: Option<Child>,
    pub busy: bool,
    pub resume: Option<String>,
}

#[derive(Default)]
pub struct Agents {
    pub sessions: Mutex<HashMap<String, Arc<tokio::sync::Mutex<Session>>>>,
}

pub fn mcp_config(root: &Path, gamma: &Path) -> Value {
    json!({"mcpServers": {"gamma": {"command": gamma, "args": ["mcp", "--project", root]}}})
}

fn which(prog: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(prog))
            .find(|f| f.is_file())
    })
}

pub fn providers() -> Value {
    json!([
        {"id": "claude", "name": "Claude Code", "installed": which("claude").is_some()},
        {"id": "codex", "name": "Codex", "installed": which("codex").is_some()},
    ])
}

impl Agents {
    pub async fn start(
        &self,
        ps: Arc<ProjectState>,
        provider: &str,
        gamma: &Path,
        extra: &[String],
    ) -> Result<Value> {
        let root = ps.root();
        let cfg_path = root.join(".gamma/local/mcp.json");
        gamma_core::fsutil::atomic_write(
            &cfg_path,
            serde_json::to_string_pretty(&mcp_config(&root, gamma))?.as_bytes(),
        )?;
        let id = format!("{provider}-{:04x}", rand::random::<u16>());
        let mut session = Session {
            provider: provider.into(),
            id: id.clone(),
            stdin: None,
            child: None,
            busy: false,
            resume: None,
        };
        match provider {
            "claude" => {
                let prog = which("claude")
                    .context("Claude Code (`claude`) is not installed or not on PATH")?;
                let mut cmd = Command::new(prog);
                cmd.args([
                    "-p",
                    "--input-format",
                    "stream-json",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--include-partial-messages",
                ])
                .arg("--mcp-config")
                .arg(&cfg_path)
                .args([
                    "--append-system-prompt",
                    SYSTEM_PROMPT,
                    "--permission-mode",
                    "acceptEdits",
                    "--allowedTools",
                    "mcp__gamma",
                ])
                .args(extra)
                .current_dir(&root)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .kill_on_drop(true);
                let mut child = cmd.spawn().context("starting claude")?;
                session.stdin = child.stdin.take();
                let stdout = child.stdout.take().unwrap();
                let stderr = child.stderr.take().unwrap();
                session.child = Some(child);
                let ps2 = ps.clone();
                let actor = format!("agent:{id}");
                let sess_ref = Arc::new(tokio::sync::Mutex::new(session));
                self.sessions
                    .lock()
                    .unwrap()
                    .insert(ps.id.clone(), sess_ref.clone());
                let sr = sess_ref.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let Ok(v) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        handle_claude(&ps2, &actor, &v, &sr).await;
                    }
                    ps2.log.push(&actor, "agent.ended", json!({}));
                });
                let ps3 = ps.clone();
                let actor2 = format!("agent:{id}");
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        ps3.log
                            .ephemeral(&actor2, "agent.stderr", json!({"line": line}));
                    }
                });
            }
            "codex" => {
                which("codex").context("Codex (`codex`) is not installed or not on PATH")?;
                self.sessions
                    .lock()
                    .unwrap()
                    .insert(ps.id.clone(), Arc::new(tokio::sync::Mutex::new(session)));
            }
            other => bail!("unknown agent '{other}': use claude or codex"),
        }
        ps.log.push(
            "person",
            "agent.started",
            json!({"session": id, "provider": provider}),
        );
        Ok(json!({"session": id, "provider": provider}))
    }

    pub async fn send(&self, ps: Arc<ProjectState>, text: &str, gamma: &Path) -> Result<()> {
        let s = self
            .sessions
            .lock()
            .unwrap()
            .get(&ps.id)
            .cloned()
            .context("no agent session in this project; start one first")?;
        let mut sess = s.lock().await;
        ps.log.push(
            "person",
            "agent.message",
            json!({"session": sess.id, "role": "user", "text": text}),
        );
        match sess.provider.as_str() {
            "claude" => {
                let msg = json!({"type": "user", "message": {"role": "user", "content": [{"type": "text", "text": text}]}});
                let stdin = sess
                    .stdin
                    .as_mut()
                    .context("the agent has exited; start a new session")?;
                stdin.write_all(format!("{msg}\n").as_bytes()).await?;
                stdin.flush().await?;
                sess.busy = true;
            }
            "codex" => {
                let root = ps.root();
                let prog = which("codex").context("codex not on PATH")?;
                let mut cmd = Command::new(prog);
                cmd.arg("exec");
                if let Some(r) = &sess.resume {
                    cmd.args(["resume", r]);
                }
                cmd.arg("--json")
                    .arg("-c")
                    .arg(format!(
                        "mcp_servers.gamma.command={}",
                        toml_str(&gamma.to_string_lossy())
                    ))
                    .arg("-c")
                    .arg(format!(
                        "mcp_servers.gamma.args=[\"mcp\",\"--project\",{}]",
                        toml_str(&root.to_string_lossy())
                    ))
                    .args(["-s", "workspace-write"])
                    .arg(format!("{SYSTEM_PROMPT}\n\n{text}"))
                    .current_dir(&root)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .kill_on_drop(true);
                let mut child = cmd.spawn()?;
                let stdout = child.stdout.take().unwrap();
                let actor = format!("agent:{}", sess.id);
                let ps2 = ps.clone();
                let sr = s.clone();
                sess.busy = true;
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let Ok(v) = serde_json::from_str::<Value>(&line) else {
                            continue;
                        };
                        if let Some(id) = v
                            .get("thread_id")
                            .or_else(|| v.get("session_id"))
                            .and_then(|x| x.as_str())
                        {
                            sr.lock().await.resume = Some(id.to_string());
                        }
                        let item = v.get("item").unwrap_or(&v);
                        let ty = item
                            .get("type")
                            .or_else(|| item.get("item_type"))
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                            if ty.contains("message") || ty.contains("reasoning") {
                                ps2.log.push(&actor, "agent.message", json!({"role": if ty.contains("reasoning") { "thinking" } else { "assistant" }, "text": t}));
                            }
                        } else if ty.contains("tool") || ty.contains("command") {
                            ps2.log.push(&actor, "agent.tool", json!({"name": item.get("tool").or_else(|| item.get("command")).cloned().unwrap_or(json!(ty)), "input": item.get("arguments").cloned().unwrap_or(Value::Null)}));
                        }
                    }
                    let _ = child.wait().await;
                    sr.lock().await.busy = false;
                    turn_end(&ps2, &actor).await;
                });
            }
            _ => {}
        }
        Ok(())
    }

    pub async fn stop(&self, ps: &ProjectState) -> Result<()> {
        let s = self.sessions.lock().unwrap().remove(&ps.id);
        if let Some(s) = s {
            let mut sess = s.lock().await;
            if let Some(c) = sess.child.as_mut() {
                let _ = c.start_kill();
            }
            ps.log
                .push("person", "agent.stopped", json!({"session": sess.id}));
        }
        Ok(())
    }

    pub fn status(&self, ps: &ProjectState) -> Value {
        match self.sessions.lock().unwrap().get(&ps.id) {
            Some(s) => match s.try_lock() {
                Ok(g) => json!({"session": g.id, "provider": g.provider, "busy": g.busy}),
                Err(_) => json!({"busy": true}),
            },
            None => json!(null),
        }
    }
}

fn toml_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

async fn turn_end(ps: &Arc<ProjectState>, actor: &str) {
    ps.log.push(actor, "agent.turn_end", json!({}));
    let root = ps.root();
    let a = actor.to_string();
    let last_prompt = ps
        .log
        .since(0, Some(&["agent.message"]))
        .into_iter()
        .rev()
        .find(|e| e.data["role"] == "user")
        .and_then(|e| e.data["text"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();
    let msg = format!(
        "Agent turn: {}",
        last_prompt
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(72)
            .collect::<String>()
    );
    let res = tokio::task::spawn_blocking(move || crate::git::checkpoint(&root, &msg, &a)).await;
    if let Ok(Ok(Some(c))) = res {
        let _ = crate::projects::save_checkpoint_layout(ps, c.n);
        ps.log.push(
            actor,
            "checkpoint",
            serde_json::to_value(&c).unwrap_or_default(),
        );
    }
}

async fn handle_claude(
    ps: &Arc<ProjectState>,
    actor: &str,
    v: &Value,
    sess: &Arc<tokio::sync::Mutex<Session>>,
) {
    match v["type"].as_str() {
        Some("stream_event") => {
            // partial text for live display
            if let Some(t) = v["event"]["delta"]["text"].as_str() {
                ps.log.ephemeral(actor, "agent.delta", json!({"text": t}));
            }
        }
        Some("assistant") => {
            for c in v["message"]["content"]
                .as_array()
                .cloned()
                .unwrap_or_default()
            {
                match c["type"].as_str() {
                    Some("text") => {
                        ps.log.push(
                            actor,
                            "agent.message",
                            json!({"role": "assistant", "text": c["text"]}),
                        );
                    }
                    Some("tool_use") => {
                        ps.log.push(
                            actor,
                            "agent.tool",
                            json!({"name": c["name"], "input": c["input"]}),
                        );
                    }
                    _ => {}
                }
            }
        }
        Some("result") => {
            sess.lock().await.busy = false;
            ps.log.push(actor, "agent.result", json!({"cost_usd": v["total_cost_usd"], "duration_ms": v["duration_ms"], "is_error": v["is_error"], "turns": v["num_turns"]}));
            turn_end(ps, actor).await;
        }
        Some("system") => {
            if v["subtype"] == "init" {
                ps.log.push(
                    actor,
                    "agent.ready",
                    json!({"model": v["model"], "tools": v["tools"].as_array().map(|a| a.len())}),
                );
            }
        }
        _ => {}
    }
}
