//! Place and route by editing a gdsfactory YAML netlist (`*.pic.yml`):
//! instances, placements, connections and routes. The file stays readable
//! and editable by hand; gdsfactory builds it (`gf.read.from_yaml`).

use anyhow::{Context, Result, bail};
use serde_yaml::{Mapping, Value};
use std::path::Path;

pub struct Netlist {
    pub doc: Value,
}

fn key(s: &str) -> Value {
    Value::String(s.to_string())
}

impl Netlist {
    pub fn new(name: &str) -> Netlist {
        let mut m = Mapping::new();
        m.insert(key("name"), key(name));
        m.insert(key("instances"), Value::Mapping(Mapping::new()));
        m.insert(key("placements"), Value::Mapping(Mapping::new()));
        m.insert(key("routes"), Value::Mapping(Mapping::new()));
        m.insert(key("ports"), Value::Mapping(Mapping::new()));
        Netlist {
            doc: Value::Mapping(m),
        }
    }

    pub fn load(path: &Path) -> Result<Netlist> {
        let t =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let doc: Value = serde_yaml::from_str(&t)
            .with_context(|| format!("{} is not valid YAML", path.display()))?;
        if !doc.is_mapping() {
            bail!(
                "{} is not a gdsfactory netlist (expected a mapping)",
                path.display()
            );
        }
        Ok(Netlist { doc })
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        crate::fsutil::atomic_write(path, serde_yaml::to_string(&self.doc)?.as_bytes())
    }

    fn section(&mut self, name: &str) -> &mut Mapping {
        let m = self.doc.as_mapping_mut().unwrap();
        if !m.get(key(name)).map(|v| v.is_mapping()).unwrap_or(false) {
            m.insert(key(name), Value::Mapping(Mapping::new()));
        }
        m.get_mut(key(name)).unwrap().as_mapping_mut().unwrap()
    }

    /// Add or replace an instance of `component` (a gdsfactory cell name) with settings.
    pub fn place(
        &mut self,
        name: &str,
        component: &str,
        settings: Option<serde_json::Value>,
        x: f64,
        y: f64,
        rotation: f64,
        mirror: bool,
    ) -> Result<()> {
        if name.is_empty() || name.contains(' ') || name.contains(',') {
            bail!("instance name '{name}' must be non-empty without spaces or commas");
        }
        let mut inst = Mapping::new();
        inst.insert(key("component"), key(component));
        if let Some(s) = settings
            && !s.is_null()
        {
            inst.insert(key("settings"), serde_yaml::to_value(s)?);
        }
        self.section("instances")
            .insert(key(name), Value::Mapping(inst));
        let mut pl = Mapping::new();
        pl.insert(key("x"), Value::from(x));
        pl.insert(key("y"), Value::from(y));
        if rotation != 0.0 {
            pl.insert(key("rotation"), Value::from(rotation));
        }
        if mirror {
            pl.insert(key("mirror"), Value::Bool(true));
        }
        self.section("placements")
            .insert(key(name), Value::Mapping(pl));
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let a = self.section("instances").remove(key(name)).is_some();
        self.section("placements").remove(key(name));
        // drop routes and connections touching it
        let prefix = format!("{name},");
        for sec in ["connections"] {
            let m = self.section(sec);
            let ks: Vec<Value> = m
                .iter()
                .filter(|(k, v)| {
                    k.as_str().map(|s| s.starts_with(&prefix)).unwrap_or(false)
                        || v.as_str().map(|s| s.starts_with(&prefix)).unwrap_or(false)
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in ks {
                m.remove(k);
            }
        }
        a
    }

    /// Route `a` (`inst,port`) to `b` with a gdsfactory router in route bundle `bundle`.
    pub fn route(
        &mut self,
        bundle: &str,
        a: &str,
        b: &str,
        cross_section: Option<&str>,
    ) -> Result<()> {
        for p in [a, b] {
            let (inst, _) = p
                .split_once(',')
                .with_context(|| format!("'{p}' should be instance,port"))?;
            if !self.section("instances").contains_key(key(inst)) {
                bail!("no instance '{inst}' in the netlist");
            }
        }
        let routes = self.section("routes");
        if !routes
            .get(key(bundle))
            .map(|v| v.is_mapping())
            .unwrap_or(false)
        {
            let mut r = Mapping::new();
            r.insert(key("links"), Value::Mapping(Mapping::new()));
            if let Some(xs) = cross_section {
                let mut s = Mapping::new();
                s.insert(key("cross_section"), key(xs));
                r.insert(key("settings"), Value::Mapping(s));
            }
            routes.insert(key(bundle), Value::Mapping(r));
        }
        let r = routes
            .get_mut(key(bundle))
            .unwrap()
            .as_mapping_mut()
            .unwrap();
        if !r.get(key("links")).map(|v| v.is_mapping()).unwrap_or(false) {
            r.insert(key("links"), Value::Mapping(Mapping::new()));
        }
        r.get_mut(key("links"))
            .unwrap()
            .as_mapping_mut()
            .unwrap()
            .insert(key(a), key(b));
        Ok(())
    }

    /// Abut `a` to `b` directly (gdsfactory `connections`).
    pub fn connect(&mut self, a: &str, b: &str) {
        self.section("connections").insert(key(a), key(b));
    }

    /// Expose an instance port as a port of the top cell.
    pub fn expose(&mut self, name: &str, inst_port: &str) {
        self.section("ports").insert(key(name), key(inst_port));
    }

    pub fn instances(&self) -> Vec<(String, String)> {
        self.doc
            .get("instances")
            .and_then(|v| v.as_mapping())
            .map(|m| {
                m.iter()
                    .map(|(k, v)| {
                        (
                            k.as_str().unwrap_or("").to_string(),
                            v.get("component")
                                .and_then(|c| c.as_str())
                                .unwrap_or("")
                                .to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_netlist() {
        let mut n = Netlist::new("top");
        n.place(
            "mmi",
            "mmi1x2",
            Some(serde_json::json!({"length_mmi": 5.5})),
            0.0,
            0.0,
            0.0,
            false,
        )
        .unwrap();
        n.place("s1", "straight", None, 30.0, 10.0, 0.0, false)
            .unwrap();
        n.route("r1", "mmi,o2", "s1,o1", Some("strip")).unwrap();
        n.expose("o1", "mmi,o1");
        assert!(n.route("r2", "x,o1", "s1,o2", None).is_err());
        let d = std::env::temp_dir().join(format!("n{}.pic.yml", std::process::id()));
        n.save(&d).unwrap();
        let m = Netlist::load(&d).unwrap();
        assert_eq!(m.instances().len(), 2);
        let t = std::fs::read_to_string(&d).unwrap();
        assert!(t.contains("mmi,o2: s1,o1"));
        let _ = std::fs::remove_file(d);
    }
}
