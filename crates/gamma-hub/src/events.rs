//! The per-project event log: append-only JSON lines, fsynced, single writer.
//! Ephemeral events (progress, field slices, streamed agent text) are
//! broadcast but not written.

use anyhow::Result;
use gamma_core::fsutil::{append_line, read_lines_repair};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Mutex;
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Sequence number in the log; 0 for ephemeral events.
    pub seq: u64,
    pub time: String,
    /// `person`, `agent:<session>`, `hub`, `cli`.
    pub actor: String,
    pub kind: String,
    pub data: Value,
}

pub fn now() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_default()
}

pub struct EventLog {
    path: PathBuf,
    events: Mutex<Vec<Event>>,
    tx: broadcast::Sender<Event>,
}

impl EventLog {
    pub fn open(path: PathBuf) -> Result<EventLog> {
        let lines = read_lines_repair(&path)?;
        let events: Vec<Event> = lines.iter().filter_map(|l| serde_json::from_str(l).ok()).collect();
        let (tx, _) = broadcast::channel(4096);
        Ok(EventLog { path, events: Mutex::new(events), tx })
    }

    pub fn last_seq(&self) -> u64 {
        self.events.lock().unwrap().last().map(|e| e.seq).unwrap_or(0)
    }

    /// Append a logged event and broadcast it.
    pub fn push(&self, actor: &str, kind: &str, data: Value) -> Event {
        let mut ev = self.events.lock().unwrap();
        let seq = ev.last().map(|e| e.seq).unwrap_or(0) + 1;
        let e = Event { seq, time: now(), actor: actor.into(), kind: kind.into(), data };
        if let Err(err) = append_line(&self.path, &serde_json::to_string(&e).unwrap_or_default()) {
            tracing::warn!("event log write failed: {err:#}");
        }
        ev.push(e.clone());
        // keep memory bounded; the file keeps everything
        let n = ev.len();
        if n > 20_000 {
            ev.drain(0..n - 10_000);
        }
        drop(ev);
        let _ = self.tx.send(e.clone());
        e
    }

    /// Broadcast without logging.
    pub fn ephemeral(&self, actor: &str, kind: &str, data: Value) {
        let _ = self.tx.send(Event { seq: 0, time: now(), actor: actor.into(), kind: kind.into(), data });
    }

    pub fn since(&self, after: u64, kinds: Option<&[&str]>) -> Vec<Event> {
        self.events.lock().unwrap().iter().filter(|e| e.seq > after && kinds.map(|k| k.contains(&e.kind.as_str())).unwrap_or(true)).cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_persists() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("events.jsonl");
        {
            let l = EventLog::open(p.clone()).unwrap();
            l.push("person", "a", serde_json::json!({"x": 1}));
            l.ephemeral("hub", "progress", serde_json::json!({}));
            l.push("agent:1", "b", serde_json::json!({}));
        }
        let l = EventLog::open(p).unwrap();
        assert_eq!(l.last_seq(), 2);
        assert_eq!(l.since(1, None).len(), 1);
    }
}
