//! Messages a worker writes while it runs, one JSON object per line.
//!
//! The same messages travel from a local worker process and, over SSH, from
//! a remote one; the hub reads them, keeps the latest state of each job and
//! forwards them to the viewers.

use base64::Engine;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Msg {
    Hello {
        version: String,
        lightsim: String,
        host_arch: String,
        devices: Vec<crate::devices::DeviceInfo>,
        precision: String,
    },
    Plan {
        cells: usize,
        steps: usize,
        dt: f64,
        memory_bytes: u64,
        device: String,
    },
    Progress {
        step: usize,
        steps: usize,
        elapsed: f64,
        decay: Option<f64>,
        mcells_per_s: f64,
    },
    Slice(Slice),
    Log {
        line: String,
    },
    Done {
        steps: usize,
        seconds: f64,
        mcells_per_s: f64,
        stopped_early: bool,
        results: String,
        backend: String,
    },
    Error {
        message: String,
    },
}

/// A field plane, f32 samples base64 encoded (row-major over the two in-plane axes).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Slice {
    pub step: usize,
    pub comp: String,
    pub axis: usize,
    pub index: usize,
    pub dims: [usize; 2],
    pub data: String,
    pub max_abs: f32,
}

impl Slice {
    pub fn new(
        step: usize,
        comp: &str,
        axis: usize,
        index: usize,
        dims: [usize; 2],
        data: &[f32],
    ) -> Slice {
        let mut bytes = Vec::with_capacity(data.len() * 4);
        let mut m = 0f32;
        for v in data {
            bytes.extend(v.to_le_bytes());
            m = m.max(v.abs());
        }
        Slice {
            step,
            comp: comp.into(),
            axis,
            index,
            dims,
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
            max_abs: m,
        }
    }

    pub fn values(&self) -> Vec<f32> {
        let b = base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .unwrap_or_default();
        b.as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }
}

impl Msg {
    pub fn line(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
    pub fn parse(line: &str) -> Option<Msg> {
        serde_json::from_str(line.trim()).ok()
    }
}

/// Parse lightsim's own progress lines (`  step 200/420 (0.4 s)`), printed by
/// its multi-GPU driver.
pub fn parse_lightsim_progress(line: &str) -> Option<(usize, usize, f64)> {
    let l = line.trim();
    let rest = l.strip_prefix("step ")?;
    let (frac, t) = rest.split_once(' ')?;
    let (a, b) = frac.split_once('/')?;
    let t = t.trim_start_matches('(').trim_end_matches(" s)").trim();
    Some((a.parse().ok()?, b.parse().ok()?, t.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let s = Slice::new(10, "Ez", 2, 5, [2, 2], &[1.0, -2.0, 0.5, 0.0]);
        let m = Msg::Slice(s.clone());
        let back = Msg::parse(&m.line()).unwrap();
        assert_eq!(back, m);
        assert_eq!(s.values(), vec![1.0, -2.0, 0.5, 0.0]);
        assert_eq!(s.max_abs, 2.0);
        assert_eq!(
            parse_lightsim_progress("  step 200/420 (0.4 s)"),
            Some((200, 420, 0.4))
        );
    }
}
