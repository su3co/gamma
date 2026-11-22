//! Layer stack and materials (`stack.toml`).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Stack {
    #[serde(default, rename = "layer")]
    pub layers: Vec<StackLayer>,
    #[serde(default, rename = "material")]
    pub materials: Vec<Material>,
    /// Wavelength the indices refer to, µm.
    #[serde(default = "default_wl")]
    pub wavelength_um: f64,
}

fn default_wl() -> f64 {
    1.55
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StackLayer {
    pub name: String,
    /// GDS layer and datatype; absent for blanket layers (substrate, box, cladding).
    #[serde(default)]
    pub gds: Option<[u16; 2]>,
    /// Bottom of the layer, µm.
    pub zmin: f64,
    pub thickness: f64,
    pub material: String,
    /// Display colour, `#rrggbb`.
    #[serde(default)]
    pub color: Option<String>,
    /// Design rules, µm.
    #[serde(default)]
    pub min_width: Option<f64>,
    #[serde(default)]
    pub min_space: Option<f64>,
    /// Sidewall angle in degrees from vertical (display only).
    #[serde(default)]
    pub sidewall_deg: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Material {
    pub name: String,
    /// Constant refractive index at the stack wavelength.
    #[serde(default)]
    pub index: Option<f64>,
    /// Name in lightsim's material library (dispersive model).
    #[serde(default)]
    pub lightsim: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

impl StackLayer {
    pub fn zmax(&self) -> f64 {
        self.zmin + self.thickness
    }
}

impl Stack {
    pub fn load(path: &Path) -> Result<Stack> {
        let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("{} is not a valid layer stack", path.display()))
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }

    pub fn layer_for(&self, layer: u16, datatype: u16) -> Option<&StackLayer> {
        self.layers.iter().find(|l| l.gds == Some([layer, datatype]))
    }

    pub fn material(&self, name: &str) -> Option<&Material> {
        self.materials.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }

    /// Index of a material at the stack wavelength (constant index if given,
    /// else a table of common PIC materials at 1550 nm).
    pub fn index_of(&self, name: &str) -> Option<f64> {
        if let Some(n) = self.material(name).and_then(|m| m.index) {
            return Some(n);
        }
        let n = match name.to_ascii_lowercase().as_str() {
            "si" | "silicon" => 3.476,
            "sio2" | "oxide" | "silica" => 1.444,
            "sin" | "si3n4" | "nitride" => 1.996,
            "air" | "vacuum" => 1.0,
            "water" | "h2o" => 1.333,
            "ln" | "linbo3" => 2.21,
            "aln" => 2.12,
            "ge" => 4.27,
            "polysilicon" => 3.48,
            "tio2" => 2.25,
            "ta2o5" => 2.07,
            "al2o3" => 1.75,
            _ => return None,
        };
        Some(n)
    }

    /// A generic 220 nm silicon-on-insulator stack (gdsfactory generic PDK layers).
    pub fn soi220() -> Stack {
        let l = |name: &str, gds: Option<[u16; 2]>, zmin: f64, t: f64, mat: &str, color: &str, w: Option<f64>, s: Option<f64>| StackLayer {
            name: name.into(),
            gds,
            zmin,
            thickness: t,
            material: mat.into(),
            color: Some(color.into()),
            min_width: w,
            min_space: s,
            sidewall_deg: None,
        };
        Stack {
            layers: vec![
                l("substrate", None, -3.0, 1.0, "si", "#4a4f5a", None, None),
                l("box", None, -2.0, 2.0, "sio2", "#9fb7c9", None, None),
                l("core", Some([1, 0]), 0.0, 0.22, "si", "#d9a441", Some(0.15), Some(0.15)),
                l("slab90", Some([3, 0]), 0.0, 0.09, "si", "#c27b2a", Some(0.15), Some(0.15)),
                l("nitride", Some([34, 0]), 0.32, 0.4, "sin", "#5fb39a", Some(0.3), Some(0.3)),
                l("clad", None, 0.0, 1.0, "sio2", "#bcd3e3", None, None),
            ],
            materials: vec![
                Material { name: "si".into(), index: Some(3.476), lightsim: Some("Si".into()), color: None },
                Material { name: "sio2".into(), index: Some(1.444), lightsim: Some("SiO2".into()), color: None },
                Material { name: "sin".into(), index: Some(1.996), lightsim: Some("Si3N4".into()), color: None },
            ],
            wavelength_um: 1.55,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let s = Stack::soi220();
        let t = s.to_toml();
        let b: Stack = toml::from_str(&t).unwrap();
        assert_eq!(s, b);
        assert_eq!(b.layer_for(1, 0).unwrap().name, "core");
        assert_eq!(b.index_of("SiO2"), Some(1.444));
    }
}
