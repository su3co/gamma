//! Simulation jobs: queued per resource (the local CPU, each local GPU, each
//! remote host), run by a `gamma worker` process (locally, or over SSH on a
//! remote host), with progress and field slices streamed back and results
//! stored in `results/<sim>/<run>/`.

use crate::events::now;
use crate::projects::ProjectState;
use crate::remote;
use anyhow::{Context, Result, bail};
use gamma_core::project::RemoteCfg;
use gamma_sim::stream::Msg;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Staging,
    Running,
    Fetching,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
pub struct Job {
    pub id: String,
    pub project: String,
    pub sim: String,
    pub run_id: String,
    /// `cpu`, `cuda:0`, ... on the target.
    pub device: String,
    /// Remote host name, or `None` for this machine.
    pub host: Option<String>,
    pub preview: bool,
    pub actor: String,
    pub status: Status,
    pub created: String,
    pub started: Option<String>,
    pub finished: Option<String>,
    pub plan: Option<Value>,
    pub progress: Option<Value>,
    pub error: Option<String>,
    pub log: Vec<String>,
    /// (step, energy / peak) samples for the decay plot.
    pub decay: Vec<(usize, f64)>,
    #[serde(skip)]
    pub last_slice: Option<Value>,
    #[serde(skip)]
    pub cancel: Option<tokio::sync::watch::Sender<bool>>,
}

impl Job {
    pub fn resource(&self) -> String {
        match &self.host {
            Some(h) => format!("remote:{h}"),
            None => format!("local:{}", self.device),
        }
    }
}

pub struct RunRequest {
    pub sim: String,
    pub device: String,
    pub host: Option<RemoteCfg>,
    pub preview: bool,
    pub actor: String,
    pub slice: Option<String>,
}

#[derive(Default)]
pub struct Jobs {
    pub jobs: Mutex<Vec<Job>>,
    busy: Mutex<HashMap<String, String>>,
}

pub fn run_id() -> String {
    let t = time::OffsetDateTime::now_utc();
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}-{:04x}",
        t.year(),
        t.month() as u8,
        t.day(),
        t.hour(),
        t.minute(),
        t.second(),
        rand::random::<u16>()
    )
}

impl Jobs {
    pub fn list(&self) -> Vec<Job> {
        self.jobs.lock().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<Job> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.id == id)
            .cloned()
    }

    pub fn last_slice(&self, id: &str) -> Option<Value> {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .find(|j| j.id == id)
            .and_then(|j| j.last_slice.clone())
    }

    pub fn active(&self) -> usize {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|j| {
                matches!(
                    j.status,
                    Status::Queued | Status::Staging | Status::Running | Status::Fetching
                )
            })
            .count()
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut Job)) -> Option<Job> {
        let mut js = self.jobs.lock().unwrap();
        let j = js.iter_mut().find(|j| j.id == id)?;
        f(j);
        Some(j.clone())
    }

    pub fn cancel(&self, id: &str) -> Result<Job> {
        let j = self
            .update(id, |j| {
                if let Some(c) = &j.cancel {
                    let _ = c.send(true);
                }
                if j.status == Status::Queued {
                    j.status = Status::Cancelled;
                    j.finished = Some(now());
                }
            })
            .context("no such job")?;
        Ok(j)
    }

    /// Queue a run of `req` in project `ps`; returns the job.
    pub fn submit(
        self: &Arc<Self>,
        ps: Arc<ProjectState>,
        req: RunRequest,
        worker: PathBuf,
    ) -> Result<Job> {
        // validate early: the spec must load
        let sim = ps.sim(&req.sim, req.preview)?;
        let (tx, rx) = tokio::sync::watch::channel(false);
        let job = Job {
            id: format!("j{}", rand::random::<u32>()),
            project: ps.id.clone(),
            sim: req.sim.clone(),
            run_id: run_id(),
            device: req.device.clone(),
            host: req.host.as_ref().map(|h| h.name.clone()),
            preview: req.preview,
            actor: req.actor.clone(),
            status: Status::Queued,
            created: now(),
            started: None,
            finished: None,
            plan: None,
            progress: None,
            error: None,
            log: vec![],
            decay: vec![],
            last_slice: None,
            cancel: Some(tx),
        };
        self.jobs.lock().unwrap().push(job.clone());
        ps.log.push(&req.actor, "sim.queued", json!({"job": job.id, "sim": job.sim, "device": job.device, "host": job.host, "preview": job.preview, "run": job.run_id}));
        let me = self.clone();
        let jid = job.id.clone();
        tokio::spawn(async move {
            let res = me
                .execute(&jid, ps.clone(), (*sim).clone(), req, worker, rx)
                .await;
            let res = match res {
                Ok(()) => me.get(&jid).map(|j| j.status).unwrap_or(Status::Done),
                Err(e) => {
                    let msg = format!("{e:#}");
                    let cancelled = me
                        .get(&jid)
                        .map(|j| j.cancel.as_ref().map(|c| *c.borrow()).unwrap_or(false))
                        .unwrap_or(false);
                    me.update(&jid, |j| {
                        j.status = if cancelled {
                            Status::Cancelled
                        } else {
                            Status::Failed
                        };
                        j.error = Some(msg.clone());
                        j.finished = Some(now());
                    });
                    if cancelled {
                        Status::Cancelled
                    } else {
                        Status::Failed
                    }
                }
            };
            if let Some(j) = me.get(&jid) {
                me.busy.lock().unwrap().remove(&j.resource());
                ps.log.push(&j.actor, "sim.done", json!({"job": j.id, "sim": j.sim, "run": j.run_id, "status": res, "error": j.error, "device": j.device, "host": j.host}));
            }
        });
        Ok(job)
    }

    async fn wait_for_resource(
        &self,
        jid: &str,
        rx: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        loop {
            let res = self.get(jid).context("job vanished")?.resource();
            {
                let mut b = self.busy.lock().unwrap();
                if !b.contains_key(&res) {
                    // first queued job for this resource (FIFO)
                    let first = self
                        .jobs
                        .lock()
                        .unwrap()
                        .iter()
                        .find(|j| j.status == Status::Queued && j.resource() == res)
                        .map(|j| j.id.clone());
                    if first.as_deref() == Some(jid) {
                        b.insert(res, jid.to_string());
                        return Ok(());
                    }
                }
            }
            if *rx.borrow() {
                bail!("cancelled while queued");
            }
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
                _ = rx.changed() => {}
            }
        }
    }

    async fn execute(
        &self,
        jid: &str,
        ps: Arc<ProjectState>,
        sim: gamma_sim::Simulation,
        req: RunRequest,
        worker: PathBuf,
        mut rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<()> {
        self.wait_for_resource(jid, &mut rx).await?;
        let job = self.get(jid).context("job vanished")?;
        if job.status == Status::Cancelled {
            return Ok(());
        }
        self.update(jid, |j| {
            j.status = Status::Staging;
            j.started = Some(now());
        });
        let dir = ps.root().join("results").join(&job.sim).join(&job.run_id);
        tokio::fs::create_dir_all(&dir).await?;
        let spec_path = dir.join("spec.json");
        // the spec as written (unknown keys pass through to the worker)
        let text = ps
            .sim_text(&job.sim, job.preview)
            .unwrap_or_else(|_| sim.to_json());
        gamma_core::fsutil::atomic_write(&spec_path, text.as_bytes())?;
        let p = ps.snapshot();
        let commit = std::process::Command::new("git")
            .arg("-C")
            .arg(&p.root)
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        let mut run = json!({
            "id": job.run_id, "sim": job.sim, "device": job.device, "host": job.host, "preview": job.preview,
            "actor": job.actor, "status": "running", "started": now(), "spec_sha256": gamma_core::fsutil::file_sha256(&spec_path).ok(),
            "commit": commit, "gamma": gamma_core::VERSION, "lightsim": gamma_sim::LIGHTSIM_REV,
        });
        gamma_core::fsutil::atomic_write(
            &dir.join("run.json"),
            serde_json::to_string_pretty(&run)?.as_bytes(),
        )?;
        let results = dir.join("results.json");
        let slice_arg = req.slice.clone().unwrap_or_else(|| "auto".into());
        let outcome = match &req.host {
            None => {
                let mut cmd = tokio::process::Command::new(&worker);
                cmd.args(["worker", "run"])
                    .arg(&spec_path)
                    .args([
                        "--device",
                        &job.device,
                        "--slice",
                        &slice_arg,
                        "--every",
                        "1",
                    ])
                    .arg("--out")
                    .arg(&results);
                self.stream(jid, &ps, cmd, &mut rx).await
            }
            Some(h) => {
                self.run_remote(jid, &ps, h, &spec_path, &results, &job, &slice_arg, &mut rx)
                    .await
            }
        };
        let status = match &outcome {
            Ok(Some(done)) => {
                run["stats"] = done.clone();
                "done"
            }
            Ok(None) => "cancelled",
            Err(_) => "failed",
        };
        run["status"] = json!(status);
        run["finished"] = json!(now());
        if let Err(e) = &outcome {
            run["error"] = json!(format!("{e:#}"));
        }
        // metrics for checkpoint result diffs
        if results.is_file() {
            if let Ok(l) = gamma_sim::results::Loaded::read(&results, &sim) {
                let sp = gamma_sim::results::sparams(&l).ok().flatten();
                run["metrics"] = gamma_sim::results::metrics(&l, sp.as_ref());
            }
        }
        gamma_core::fsutil::atomic_write(
            &dir.join("run.json"),
            serde_json::to_string_pretty(&run)?.as_bytes(),
        )?;
        // results.json and the spec copy can be large: keep them out of git
        let gi = dir.join(".gitignore");
        if !gi.exists() {
            let _ = std::fs::write(gi, "results.json\nspec.json\n");
        }
        match outcome {
            Ok(Some(_)) => {
                self.update(jid, |j| {
                    j.status = Status::Done;
                    j.finished = Some(now());
                });
                Ok(())
            }
            Ok(None) => {
                self.update(jid, |j| {
                    j.status = Status::Cancelled;
                    j.finished = Some(now());
                });
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_remote(
        &self,
        jid: &str,
        ps: &Arc<ProjectState>,
        h: &RemoteCfg,
        spec: &std::path::Path,
        results: &std::path::Path,
        job: &Job,
        slice: &str,
        rx: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<Value>> {
        let wd = format!("{}/{}", remote::workdir(h), job.id);
        self.log_line(jid, ps, format!("staging the spec on {}", h.name));
        remote::put(h, spec, &format!("{wd}/spec.json")).await?;
        let prog = remote::program(h);
        // `cuda:any` lets the launcher pick a free card; with `renumber_gpus`
        // the cards the launcher grants are cuda:0.. inside the job
        let requested = job.device.strip_prefix("cuda:").unwrap_or("").to_string();
        let worker_device =
            if job.device.starts_with("cuda") && (h.renumber_gpus || requested == "any") {
                let k = if requested == "any" || requested.is_empty() {
                    1
                } else {
                    requested.split(',').count()
                };
                format!(
                    "cuda:{}",
                    (0..k).map(|i| i.to_string()).collect::<Vec<_>>().join(",")
                )
            } else {
                job.device.clone()
            };
        let worker_cmd = format!(
            "{} worker run {} --device {} --slice {} --every 1 --out {}",
            remote::sh_quote(&prog),
            remote::sh_quote(&format!("{wd}/spec.json")),
            remote::sh_quote(&worker_device),
            remote::sh_quote(slice),
            remote::sh_quote(&format!("{wd}/results.json"))
        );
        let gpus = if requested.is_empty() {
            "0".to_string()
        } else {
            requested.clone()
        };
        let label = format!("gamma {} {}", job.sim, job.run_id);
        let cmd = match &h.launcher {
            Some(l) if l.contains("{host}") => {
                // a local wrapper that reaches the host itself (a queue helper)
                // placeholders are filled after splitting, so values may contain spaces
                let mut parts: Vec<String> = shell_split(l)
                    .into_iter()
                    .map(|t| {
                        t.replace("{host}", &h.ssh)
                            .replace("{gpus}", &gpus)
                            .replace("{label}", &label)
                    })
                    .collect();
                // the wrapper quotes each argument for the host's shell: hand it
                // one `bash -c` so `~` and quoting in the worker command work
                parts.extend(["bash".to_string(), "-c".to_string(), worker_cmd.clone()]);
                let mut c = tokio::process::Command::new(&parts[0]);
                c.args(&parts[1..]);
                c
            }
            Some(l) => {
                let line = l
                    .replace("{gpus}", &gpus)
                    .replace("{label}", &remote::sh_quote(&label));
                let mut c = remote::ssh(h);
                c.arg(format!("{line} {worker_cmd}"));
                c
            }
            None => {
                let mut c = remote::ssh(h);
                c.arg(worker_cmd.clone());
                c
            }
        };
        let done = self.stream(jid, ps, cmd, rx).await?;
        if done.is_some() {
            self.update(jid, |j| j.status = Status::Fetching);
            self.log_line(jid, ps, format!("fetching results from {}", h.name));
            remote::get(h, &format!("{wd}/results.json"), results).await?;
            let _ = remote::run_remote(h, &format!("rm -rf {}", remote::sh_quote(&wd))).await;
        }
        Ok(done)
    }

    fn log_line(&self, jid: &str, ps: &Arc<ProjectState>, line: String) {
        self.update(jid, |j| {
            j.log.push(line.clone());
            if j.log.len() > 200 {
                j.log.remove(0);
            }
        });
        ps.log
            .ephemeral("hub", "sim.log", json!({"job": jid, "line": line}));
    }

    /// Run a worker command and follow its stream. `Ok(None)` if cancelled.
    async fn stream(
        &self,
        jid: &str,
        ps: &Arc<ProjectState>,
        mut cmd: tokio::process::Command,
        rx: &mut tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<Value>> {
        cmd.stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd.spawn().context("starting the worker")?;
        self.update(jid, |j| j.status = Status::Running);
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut out_lines = BufReader::new(stdout).lines();
        let mut err_lines = BufReader::new(stderr).lines();
        let mut done: Option<Value> = None;
        let mut error: Option<String> = None;
        let mut err_tail: Vec<String> = vec![];
        let mut out_open = true;
        let mut err_open = true;
        while out_open || err_open {
            tokio::select! {
                l = out_lines.next_line(), if out_open => match l? {
                    None => out_open = false,
                    Some(line) => match Msg::parse(&line) {
                        Some(Msg::Plan { cells, steps, dt, memory_bytes, device }) => {
                            let v = json!({"cells": cells, "steps": steps, "dt": dt, "memory_bytes": memory_bytes, "device": device});
                            self.update(jid, |j| j.plan = Some(v.clone()));
                            ps.log.ephemeral("hub", "sim.plan", json!({"job": jid, "plan": v}));
                        }
                        Some(Msg::Progress { step, steps, elapsed, decay, mcells_per_s }) => {
                            let v = json!({"step": step, "steps": steps, "elapsed": elapsed, "decay": decay, "mcells_per_s": mcells_per_s});
                            self.update(jid, |j| {
                                j.progress = Some(v.clone());
                                if let Some(d) = decay {
                                    if j.decay.len() < 5000 {
                                        j.decay.push((step, d));
                                    }
                                }
                            });
                            ps.log.ephemeral("hub", "sim.progress", json!({"job": jid, "progress": v}));
                        }
                        Some(Msg::Slice(s)) => {
                            let v = serde_json::to_value(&s).unwrap_or_default();
                            self.update(jid, |j| j.last_slice = Some(v.clone()));
                            ps.log.ephemeral("hub", "sim.slice", json!({"job": jid, "slice": v}));
                        }
                        Some(Msg::Done { steps, seconds, mcells_per_s, stopped_early, backend, .. }) => {
                            done = Some(json!({"steps": steps, "seconds": seconds, "mcells_per_s": mcells_per_s, "stopped_early": stopped_early, "backend": backend}));
                        }
                        Some(Msg::Error { message }) => error = Some(message),
                        Some(Msg::Log { line }) => self.log_line(jid, ps, line),
                        Some(Msg::Hello { .. }) => {}
                        None => self.log_line(jid, ps, line),
                    }
                },
                l = err_lines.next_line(), if err_open => match l? {
                    None => err_open = false,
                    Some(line) => {
                        if let Some((step, steps, el)) = gamma_sim::stream::parse_lightsim_progress(&line) {
                            let v = json!({"step": step, "steps": steps, "elapsed": el, "decay": null, "mcells_per_s": null});
                            self.update(jid, |j| j.progress = Some(v.clone()));
                            ps.log.ephemeral("hub", "sim.progress", json!({"job": jid, "progress": v}));
                        } else {
                            err_tail.push(line.clone());
                            if err_tail.len() > 30 { err_tail.remove(0); }
                            self.log_line(jid, ps, line);
                        }
                    }
                },
                _ = rx.changed() => {
                    if *rx.borrow() {
                        let _ = child.start_kill();
                        let _ = child.wait().await;
                        return Ok(None);
                    }
                }
            }
        }
        let st = child.wait().await?;
        if let Some(e) = error {
            bail!("{e}");
        }
        if st.code() == Some(75) {
            bail!(
                "the host's job queue reported it busy (exit 75); try again later or another host"
            );
        }
        if !st.success() || done.is_none() {
            bail!(
                "the worker exited with {st}{}",
                if err_tail.is_empty() {
                    String::new()
                } else {
                    format!(":\n{}", err_tail.join("\n"))
                }
            );
        }
        Ok(done)
    }
}

/// Split a command line on whitespace, honouring single and double quotes.
pub fn shell_split(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut q: Option<char> = None;
    let mut any = false;
    for c in s.chars() {
        match (q, c) {
            (Some(x), c) if c == x => q = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                q = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() || any {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if !cur.is_empty() || any {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn split() {
        assert_eq!(shell_split("a b 'c d' \"e\""), vec!["a", "b", "c d", "e"]);
        assert_eq!(
            shell_split("q run {host} --label 'x y' --"),
            vec!["q", "run", "{host}", "--label", "x y", "--"]
        );
    }
}
