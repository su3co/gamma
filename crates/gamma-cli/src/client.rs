//! A blocking client of the hub for the CLI and the MCP server.

use anyhow::{Context, Result, bail};
use gamma_hub::HubInfo;
use serde_json::{Value, json};
use std::path::Path;
use std::time::Duration;

pub struct Client {
    pub info: HubInfo,
    http: reqwest::blocking::Client,
    pub actor: String,
}

impl Client {
    /// Connect to the running hub, starting one if needed.
    pub fn connect(actor: &str) -> Result<Client> {
        let exe = std::env::current_exe()?;
        let info = gamma_hub::connect_or_spawn(&exe)?;
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(3600))
            .build()?;
        Ok(Client {
            info,
            http,
            actor: actor.into(),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.info.url(), path)
    }

    fn check(r: reqwest::blocking::Response) -> Result<reqwest::blocking::Response> {
        if r.status().is_success() {
            return Ok(r);
        }
        let code = r.status();
        let t = r.text().unwrap_or_default();
        let msg = serde_json::from_str::<Value>(&t)
            .ok()
            .and_then(|v| v["error"].as_str().map(|s| s.to_string()))
            .unwrap_or(t);
        bail!("{msg} (hub answered {code})")
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        let r = self
            .http
            .get(self.url(path))
            .bearer_auth(&self.info.token)
            .header("x-gamma-actor", &self.actor)
            .send()
            .context("talking to the hub")?;
        Ok(Self::check(r)?.json()?)
    }

    pub fn get_bytes(&self, path: &str) -> Result<Vec<u8>> {
        let r = self
            .http
            .get(self.url(path))
            .bearer_auth(&self.info.token)
            .header("x-gamma-actor", &self.actor)
            .send()
            .context("talking to the hub")?;
        Ok(Self::check(r)?.bytes()?.to_vec())
    }

    pub fn post(&self, path: &str, body: Value) -> Result<Value> {
        let r = self
            .http
            .post(self.url(path))
            .bearer_auth(&self.info.token)
            .header("x-gamma-actor", &self.actor)
            .json(&body)
            .send()
            .context("talking to the hub")?;
        Ok(Self::check(r)?.json()?)
    }

    pub fn put(&self, path: &str, body: Value) -> Result<Value> {
        let r = self
            .http
            .put(self.url(path))
            .bearer_auth(&self.info.token)
            .header("x-gamma-actor", &self.actor)
            .json(&body)
            .send()
            .context("talking to the hub")?;
        Ok(Self::check(r)?.json()?)
    }

    pub fn delete(&self, path: &str) -> Result<Value> {
        let r = self
            .http
            .delete(self.url(path))
            .bearer_auth(&self.info.token)
            .header("x-gamma-actor", &self.actor)
            .send()
            .context("talking to the hub")?;
        Ok(Self::check(r)?.json()?)
    }

    /// Open the project containing `dir`; returns its id.
    pub fn open(&self, dir: &Path) -> Result<String> {
        let dir = dir
            .canonicalize()
            .with_context(|| format!("no such folder {}", dir.display()))?;
        let v = self.post("/api/open", json!({"path": dir}))?;
        v["id"]
            .as_str()
            .map(|s| s.to_string())
            .context("hub did not return a project id")
    }

    /// Wait for a job to finish; calls `tick` with the job on every poll.
    pub fn wait_job(
        &self,
        id: &str,
        timeout: Option<Duration>,
        mut tick: impl FnMut(&Value),
    ) -> Result<Value> {
        let t0 = std::time::Instant::now();
        loop {
            let j = self.get(&format!("/api/jobs/{id}"))?;
            tick(&j);
            match j["status"].as_str() {
                Some("done") | Some("failed") | Some("cancelled") => return Ok(j),
                _ => {}
            }
            if let Some(t) = timeout
                && t0.elapsed() > t
            {
                return Ok(j);
            }
            std::thread::sleep(Duration::from_millis(700));
        }
    }
}

pub fn enc(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~/:,".contains(&b) {
            o.push(b as char);
        } else {
            o.push_str(&format!("%{b:02X}"));
        }
    }
    o
}
