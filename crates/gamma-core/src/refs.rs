//! Stable text references to objects, shared by the interface, the event log,
//! the MCP tools and the conversation with the agent.
//!
//! `cell:ring`, `layer:1/0`, `port:o1`, `poly:1/0#12`, `sim:ringdown`,
//! `monitor:ringdown/through`, `source:ringdown/dipole`, `structure:ringdown/3`,
//! `run:ringdown/20261122-153000`, `point:1.25,-3.0` (µm).

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ObjRef {
    Cell { name: String },
    Layer { layer: u16, datatype: u16 },
    Port { name: String },
    Poly { layer: u16, datatype: u16, index: usize },
    Sim { name: String },
    Monitor { sim: String, name: String },
    Source { sim: String, name: String },
    Structure { sim: String, index: usize },
    Run { sim: String, id: String },
    Point { x: f64, y: f64 },
    File { path: String },
}

impl ObjRef {
    pub fn parse(s: &str) -> Option<ObjRef> {
        let (k, v) = s.split_once(':')?;
        let ld = |t: &str| -> Option<(u16, u16)> {
            let (a, b) = t.split_once('/').unwrap_or((t, "0"));
            Some((a.parse().ok()?, b.parse().ok()?))
        };
        let pair = |t: &str| -> Option<(String, String)> {
            let (a, b) = t.split_once('/')?;
            Some((a.to_string(), b.to_string()))
        };
        Some(match k {
            "cell" => ObjRef::Cell { name: v.into() },
            "layer" => {
                let (layer, datatype) = ld(v)?;
                ObjRef::Layer { layer, datatype }
            }
            "port" => ObjRef::Port { name: v.into() },
            "poly" => {
                let (l, i) = v.split_once('#')?;
                let (layer, datatype) = ld(l)?;
                ObjRef::Poly { layer, datatype, index: i.parse().ok()? }
            }
            "sim" => ObjRef::Sim { name: v.into() },
            "monitor" => {
                let (sim, name) = pair(v)?;
                ObjRef::Monitor { sim, name }
            }
            "source" => {
                let (sim, name) = pair(v)?;
                ObjRef::Source { sim, name }
            }
            "structure" => {
                let (sim, i) = pair(v)?;
                ObjRef::Structure { sim, index: i.parse().ok()? }
            }
            "run" => {
                let (sim, id) = pair(v)?;
                ObjRef::Run { sim, id }
            }
            "point" => {
                let (x, y) = v.split_once(',')?;
                ObjRef::Point { x: x.trim().parse().ok()?, y: y.trim().parse().ok()? }
            }
            "file" => ObjRef::File { path: v.into() },
            _ => return None,
        })
    }
}

impl fmt::Display for ObjRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObjRef::Cell { name } => write!(f, "cell:{name}"),
            ObjRef::Layer { layer, datatype } => write!(f, "layer:{layer}/{datatype}"),
            ObjRef::Port { name } => write!(f, "port:{name}"),
            ObjRef::Poly { layer, datatype, index } => write!(f, "poly:{layer}/{datatype}#{index}"),
            ObjRef::Sim { name } => write!(f, "sim:{name}"),
            ObjRef::Monitor { sim, name } => write!(f, "monitor:{sim}/{name}"),
            ObjRef::Source { sim, name } => write!(f, "source:{sim}/{name}"),
            ObjRef::Structure { sim, index } => write!(f, "structure:{sim}/{index}"),
            ObjRef::Run { sim, id } => write!(f, "run:{sim}/{id}"),
            ObjRef::Point { x, y } => write!(f, "point:{x},{y}"),
            ObjRef::File { path } => write!(f, "file:{path}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        for s in ["cell:ring", "layer:1/0", "port:o1", "poly:1/0#12", "monitor:ringdown/through", "point:1.25,-3", "run:a/b"] {
            let r = ObjRef::parse(s).unwrap();
            assert_eq!(r.to_string(), s);
        }
        assert!(ObjRef::parse("nope:1").is_none());
    }
}
