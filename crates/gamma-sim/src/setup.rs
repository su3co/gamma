//! What a simulation file describes, as the set-up view and the tools show it.

use crate::devices::DeviceInfo;
use anyhow::{Context, Result};
use lightsim_core::Simulation;
use lightsim_core::geometry::Shape;
use lightsim_core::grid::{Component, Grid, MeshSpec};
use lightsim_core::raster::{RasterOptions, rasterize_component, resolve};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

pub fn load(path: &Path) -> Result<Simulation> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let sim = Simulation::from_json(&text)
        .with_context(|| format!("{} is not a lightsim simulation file", path.display()))?;
    sim.validate()
        .with_context(|| format!("{} does not describe a valid simulation", path.display()))?;
    Ok(sim)
}

/// A coarser, shorter variant of a simulation for quick looks.
pub fn preview(sim: &Simulation, mesh_scale: f64, run_time_scale: f64) -> Simulation {
    let mut s = sim.clone();
    let k = mesh_scale.max(1e-3);
    match &mut s.mesh {
        MeshSpec::Uniform { dx } => {
            for d in dx.iter_mut() {
                *d *= k;
            }
        }
        MeshSpec::Auto {
            steps_per_wavelength,
            min_step,
            max_step,
            overrides,
            ..
        } => {
            *steps_per_wavelength /= k;
            if let Some(m) = min_step {
                *m *= k;
            }
            if let Some(m) = max_step {
                *m *= k;
            }
            for o in overrides.iter_mut() {
                for d in o.dx.iter_mut() {
                    *d *= k;
                }
            }
        }
        MeshSpec::Explicit { x, y, z } => {
            let step = k.round().max(1.0) as usize;
            for v in [x, y, z] {
                let last = *v.last().unwrap();
                let mut w: Vec<f64> = v.iter().step_by(step).copied().collect();
                if *w.last().unwrap() != last {
                    w.push(last);
                }
                *v = w;
            }
        }
    }
    s.run_time *= run_time_scale;
    s
}

#[derive(Clone, Debug, Serialize)]
pub struct GridInfo {
    /// Cells per axis including absorbing layers.
    pub n: [usize; 3],
    pub cells: usize,
    pub dt: f64,
    pub steps: usize,
    pub min_cell: [f64; 3],
    pub max_cell: [f64; 3],
    pub pml_layers: [[usize; 2]; 3],
    /// Grid extent including the absorbing layers.
    pub min: [f64; 3],
    pub max: [f64; 3],
    pub memory_bytes: u64,
    pub dimensions: u8,
}

#[derive(Clone, Debug, Serialize)]
pub struct Estimate {
    pub device: String,
    pub name: String,
    pub seconds: f64,
    pub fits: Option<bool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Object {
    pub reference: String,
    pub kind: &'static str,
    #[serde(rename = "type")]
    pub ty: String,
    pub name: String,
    pub min: [f64; 3],
    pub max: [f64; 3],
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medium: Option<String>,
    /// Axis of a plane object (sources and monitors on a plane).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub axis: Option<usize>,
    /// Other parameters, light (no mode profiles or sample arrays).
    pub params: Value,
}

#[derive(Clone, Debug, Serialize)]
pub struct Medium {
    pub name: String,
    pub eps: [f64; 3],
    pub index: f64,
    pub dispersive: bool,
    pub lossy: bool,
    pub pec: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub name: String,
    pub region: [[f64; 3]; 2],
    pub grid: GridInfo,
    pub background: String,
    pub media: Vec<Medium>,
    pub objects: Vec<Object>,
    pub boundaries: Value,
    pub shutoff: Option<f64>,
    pub wavelengths: Option<[f64; 2]>,
    pub estimates: Vec<Estimate>,
}

fn grid_info(sim: &Simulation, grid: &Grid) -> Result<GridInfo> {
    let n = grid.n();
    let dt = sim.time_step(grid)?;
    let steps = (sim.run_time / dt).ceil() as usize;
    let mut minc = [f64::INFINITY; 3];
    let mut maxc = [0.0f64; 3];
    for a in 0..3 {
        for i in 0..n[a] {
            let d = grid.cell_size(a, i);
            minc[a] = minc[a].min(d);
            maxc[a] = maxc[a].max(d);
        }
    }
    let mut pml = [[0usize; 2]; 3];
    for a in 0..3 {
        let b = sim.boundaries.axis(a);
        pml[a] = [b[0].outside_layers(), b[1].outside_layers()];
    }
    Ok(GridInfo {
        n,
        cells: grid.num_cells(),
        dt,
        steps,
        min_cell: minc,
        max_cell: maxc,
        pml_layers: pml,
        min: grid.min(),
        max: grid.max(),
        memory_bytes: memory_estimate(sim, grid),
        dimensions: sim.dimensions,
    })
}

/// Device memory a run needs, approximately: fields and update coefficients
/// per cell, absorbing-layer auxiliaries, dispersive and frequency-domain
/// monitor storage.
pub fn memory_estimate(sim: &Simulation, grid: &Grid) -> u64 {
    let r = if lightsim_core::REAL_IS_F64 { 8.0 } else { 4.0 };
    let cells = grid.num_cells() as f64;
    let media = sim.media_table().unwrap_or_default();
    let lossy = media.iter().any(|m| m.is_lossy());
    let disp_poles: usize = media.iter().map(|m| m.poles.len()).max().unwrap_or(0);
    let mut per_cell = 9.0 + if lossy { 3.0 } else { 0.0 } + 2.0 /* material index */ / r;
    per_cell += 6.0 * disp_poles as f64 * 0.5; // dispersive cells are usually a fraction of the grid
    let n = grid.n();
    let mut pml_cells = 0.0;
    for a in 0..3 {
        let b = sim.boundaries.axis(a);
        let layers = (b[0].pml_layers() + b[1].pml_layers()) as f64;
        pml_cells += layers * (cells / n[a].max(1) as f64);
    }
    let mut mon = 0.0;
    let cells_in = |mn: &[f64; 3], mx: &[f64; 3]| -> f64 {
        let mut c = 1.0;
        for a in 0..3 {
            let i0 = grid.cell_index(a, mn[a]);
            let i1 = grid.cell_index(a, mx[a]);
            c *= (i1.saturating_sub(i0) + 1) as f64;
        }
        c
    };
    use lightsim_core::monitors::Monitor as M;
    for m in &sim.monitors {
        let (c, nf, comps) = match m {
            M::DftField {
                min,
                max,
                freqs,
                components,
                ..
            } => (cells_in(min, max), freqs.len(), components.len()),
            M::Flux {
                min, max, freqs, ..
            }
            | M::Mode {
                min, max, freqs, ..
            } => (cells_in(min, max), freqs.len(), 4),
            M::FluxBox {
                min, max, freqs, ..
            }
            | M::FarField {
                min, max, freqs, ..
            } => {
                let mut area = 0.0;
                for a in 0..3 {
                    let mut hi = *max;
                    hi[a] = min[a];
                    area += 2.0 * cells_in(min, &hi);
                }
                (area, freqs.len(), 4)
            }
            _ => (0.0, 0, 0),
        };
        mon += c * nf as f64 * comps as f64 * 2.0;
    }
    ((cells * per_cell + pml_cells * 4.0 + mon) * r) as u64
}

fn bbox3(v: &Value, region: &[[f64; 3]; 2]) -> Option<([f64; 3], [f64; 3])> {
    let arr = |k: &str| -> Option<[f64; 3]> {
        let a = v.get(k)?.as_array()?;
        Some([
            a.first()?.as_f64()?,
            a.get(1)?.as_f64()?,
            a.get(2)?.as_f64()?,
        ])
    };
    if let (Some(mn), Some(mx)) = (arr("min"), arr("max")) {
        let mut mn = mn;
        let mut mx = mx;
        if let (Some(ax), Some(pos)) = (
            v.get("axis").and_then(|a| a.as_u64()),
            v.get("position").and_then(|p| p.as_f64()),
        ) {
            mn[ax as usize] = pos;
            mx[ax as usize] = pos;
        }
        return Some((mn, mx));
    }
    if let Some(p) = arr("position").or_else(|| arr("center")) {
        if let (Some(ax), Some(pos)) = (
            v.get("axis").and_then(|a| a.as_u64()),
            v.get("position").and_then(|p| p.as_f64()),
        ) {
            // a plane across the region
            let mut mn = region[0];
            let mut mx = region[1];
            mn[ax as usize] = pos;
            mx[ax as usize] = pos;
            return Some((mn, mx));
        }
        return Some((p, p));
    }
    if let (Some(ax), Some(pos)) = (
        v.get("axis").and_then(|a| a.as_u64()),
        v.get("position").and_then(|p| p.as_f64()),
    ) {
        let mut mn = region[0];
        let mut mx = region[1];
        mn[ax as usize] = pos;
        mx[ax as usize] = pos;
        return Some((mn, mx));
    }
    None
}

/// Drop heavy arrays (mode profiles, sample grids, vertices) from a JSON object.
fn light(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut o = serde_json::Map::new();
            for (k, x) in m {
                match k.as_str() {
                    "profile" | "modes" | "eps" | "eps_axes" | "mu" | "e_re" | "e_im" | "h_re"
                    | "h_im" | "director" | "values" | "times" => {
                        o.insert(k.clone(), Value::String("…".into()));
                    }
                    "vertices" => {
                        o.insert(k.clone(), json!(x.as_array().map(|a| a.len()).unwrap_or(0)));
                    }
                    "freqs" | "theta" | "phi" => {
                        let a = x.as_array().cloned().unwrap_or_default();
                        if a.len() > 6 {
                            o.insert(
                                k.clone(),
                                json!({"count": a.len(), "first": a.first(), "last": a.last()}),
                            );
                        } else {
                            o.insert(k.clone(), x.clone());
                        }
                    }
                    _ => {
                        o.insert(k.clone(), light(x));
                    }
                }
            }
            Value::Object(o)
        }
        Value::Array(a) => Value::Array(a.iter().map(light).collect()),
        _ => v.clone(),
    }
}

/// Wavelength range (m) covered by the monitors, or by the source spectra.
fn wavelengths(sim: &Simulation) -> Option<[f64; 2]> {
    use lightsim_core::monitors::Monitor as M;
    let mut fs: Vec<f64> = vec![];
    for m in &sim.monitors {
        match m {
            M::DftField { freqs, .. }
            | M::Flux { freqs, .. }
            | M::FluxBox { freqs, .. }
            | M::Mode { freqs, .. }
            | M::FarField { freqs, .. } => fs.extend(freqs),
            _ => {}
        }
    }
    if fs.is_empty() {
        for s in &sim.sources {
            if let Some(f) = s.signal().freq0() {
                fs.push(f);
            }
        }
    }
    let fmin = fs.iter().cloned().fold(f64::INFINITY, f64::min);
    let fmax = fs.iter().cloned().fold(0.0, f64::max);
    if fmax <= 0.0 {
        return None;
    }
    let c = lightsim_core::constants::C0;
    Some([c / fmax, c / fmin])
}

pub fn summarize(name: &str, sim: &Simulation, devices: &[DeviceInfo]) -> Result<Summary> {
    let grid = sim.build_grid()?;
    let gi = grid_info(sim, &grid)?;
    let region = [sim.region.min, sim.region.max];
    let v = serde_json::to_value(sim)?;
    let mut objects = vec![];
    let media_table = sim.media_table()?;
    for (i, st) in sim.structures.iter().enumerate() {
        let (mn, mx) = st.shape.bbox();
        let sv = &v["structures"][i];
        let ty = sv["shape"]["type"].as_str().unwrap_or("shape").to_string();
        objects.push(Object {
            reference: format!("structure:{name}/{i}"),
            kind: "structure",
            ty,
            name: if st.name.is_empty() {
                format!("structure {i}")
            } else {
                st.name.clone()
            },
            min: clamp3(mn, &region),
            max: clamp3(mx, &region),
            medium: Some(st.medium.clone()),
            axis: match &st.shape {
                Shape::PolySlab { axis, .. } => Some(*axis),
                _ => None,
            },
            params: light(&sv["shape"]),
        });
    }
    for (i, s) in sim.sources.iter().enumerate() {
        let sv = &v["sources"][i];
        let (mn, mx) = bbox3(sv, &region).unwrap_or((region[0], region[1]));
        let nm = if s.name().is_empty() {
            format!("source{i}")
        } else {
            s.name().to_string()
        };
        objects.push(Object {
            reference: format!("source:{name}/{nm}"),
            kind: "source",
            ty: sv["type"].as_str().unwrap_or("source").into(),
            name: nm,
            min: mn,
            max: mx,
            medium: None,
            axis: sv.get("axis").and_then(|a| a.as_u64()).map(|a| a as usize),
            params: light(sv),
        });
    }
    for (i, m) in sim.monitors.iter().enumerate() {
        let mv = &v["monitors"][i];
        let (mn, mx) = bbox3(mv, &region).unwrap_or((region[0], region[1]));
        objects.push(Object {
            reference: format!("monitor:{name}/{}", m.name()),
            kind: "monitor",
            ty: mv["type"].as_str().unwrap_or("monitor").into(),
            name: m.name().to_string(),
            min: mn,
            max: mx,
            medium: None,
            axis: mv.get("axis").and_then(|a| a.as_u64()).map(|a| a as usize),
            params: light(mv),
        });
    }
    let media = media_table
        .iter()
        .map(|m| Medium {
            name: m.name.clone(),
            eps: m.eps,
            index: (m.eps[0].max(m.eps[1]).max(m.eps[2])).max(0.0).sqrt(),
            dispersive: m.is_dispersive(),
            lossy: m.is_lossy(),
            pec: m.pec,
        })
        .collect();
    let estimates = estimate(&gi, devices);
    Ok(Summary {
        name: if sim.name.is_empty() {
            name.to_string()
        } else {
            sim.name.clone()
        },
        region,
        grid: gi,
        background: sim.background.clone(),
        media,
        objects,
        boundaries: v["boundaries"].clone(),
        shutoff: sim.shutoff,
        wavelengths: wavelengths(sim),
        estimates,
    })
}

fn clamp3(p: [f64; 3], r: &[[f64; 3]; 2]) -> [f64; 3] {
    [
        p[0].clamp(r[0][0], r[1][0]),
        p[1].clamp(r[0][1], r[1][1]),
        p[2].clamp(r[0][2], r[1][2]),
    ]
}

pub fn estimate(g: &GridInfo, devices: &[DeviceInfo]) -> Vec<Estimate> {
    devices
        .iter()
        .map(|d| Estimate {
            device: d.id.clone(),
            name: d.name.clone(),
            seconds: g.cells as f64 * g.steps as f64 / d.throughput.max(1.0),
            fits: d.memory.map(|m| (g.memory_bytes as f64) < 0.9 * m as f64),
        })
        .collect()
}

/// Relative permittivity on a grid plane, from lightsim's rasteriser.
#[derive(Clone, Debug, Serialize)]
pub struct EpsSlice {
    /// Normal axis and the coordinate of the plane (m).
    pub axis: usize,
    pub coord: f64,
    /// The two in-plane axes, in increasing order.
    pub axes: [usize; 2],
    /// Samples along the in-plane axes (after any decimation).
    pub dims: [usize; 2],
    /// Cell-centre coordinates of the samples (m).
    pub u: Vec<f64>,
    pub v: Vec<f64>,
    /// Grid node coordinates along the in-plane axes (mesh lines, m).
    pub u_nodes: Vec<f64>,
    pub v_nodes: Vec<f64>,
    /// eps averaged over the in-plane E components, row-major [u][v].
    pub eps: Vec<f32>,
    /// Dominant medium per sample (index into `media`).
    pub medium: Vec<u16>,
    pub media: Vec<String>,
    pub eps_range: [f32; 2],
}

/// Rasterise the plane normal to `axis` through `coord` with lightsim's
/// rasteriser on lightsim's grid. Planes with more than `max_dim` samples
/// along an axis are decimated for display.
pub fn eps_slice(sim: &Simulation, axis: usize, coord: f64, max_dim: usize) -> Result<EpsSlice> {
    let sim = &*sim.canonical();
    let grid = sim.build_grid()?;
    let n = grid.n();
    let k = grid.cell_index(axis, coord).min(n[axis] - 1);
    let mut edges: [Vec<f64>; 3] = [
        grid.edges(0).to_vec(),
        grid.edges(1).to_vec(),
        grid.edges(2).to_vec(),
    ];
    edges[axis] = vec![grid.edges(axis)[k], grid.edges(axis)[k + 1]];
    let sub = Grid::from_edges(edges[0].clone(), edges[1].clone(), edges[2].clone());
    let media = sim.media_table()?;
    let background = media
        .iter()
        .position(|m| m.name == sim.background)
        .context("background medium missing")?;
    let structures = resolve(&sim.structures, &media)?;
    let mut ropts = RasterOptions::default();
    ropts.smoothing = sim.subpixel;
    ropts.slice = sim.raster_slice(&grid);
    let axes = match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    let sn = sub.n();
    let (nu, nv) = (sn[axes[0]], sn[axes[1]]);
    let mut eps = vec![0f32; nu * nv];
    let mut med = vec![0u16; nu * nv];
    let comps: Vec<Component> = axes.iter().map(|&a| Component::e(a)).collect();
    for (ci, &c) in comps.iter().enumerate() {
        let cm = rasterize_component(&sub, c, 0, sn[0], &structures, &media, background, &ropts);
        for iu in 0..nu {
            for iv in 0..nv {
                let mut ijk = [0usize; 3];
                ijk[axes[0]] = iu;
                ijk[axes[1]] = iv;
                let idx = cm.idx(ijk[0], ijk[1], ijk[2]);
                eps[iu * nv + iv] += cm.value[idx] as f32 / comps.len() as f32;
                if ci == 0 {
                    med[iu * nv + iv] = cm.medium[idx];
                }
            }
        }
    }
    let centers = |a: usize| -> Vec<f64> { (0..sn[a]).map(|i| sub.center(a, i)).collect() };
    let (mut u, mut v) = (centers(axes[0]), centers(axes[1]));
    // decimate for display
    let su = nu.div_ceil(max_dim.max(1)).max(1);
    let sv = nv.div_ceil(max_dim.max(1)).max(1);
    let (mut eps_d, mut med_d) = (eps, med);
    if su > 1 || sv > 1 {
        let du = nu.div_ceil(su);
        let dv = nv.div_ceil(sv);
        let mut e2 = vec![0f32; du * dv];
        let mut m2 = vec![0u16; du * dv];
        for i in 0..du {
            for j in 0..dv {
                e2[i * dv + j] = eps_d[(i * su) * nv + j * sv];
                m2[i * dv + j] = med_d[(i * su) * nv + j * sv];
            }
        }
        eps_d = e2;
        med_d = m2;
        u = u.into_iter().step_by(su).collect();
        v = v.into_iter().step_by(sv).collect();
    }
    let lo = eps_d.iter().cloned().fold(f32::INFINITY, f32::min);
    let hi = eps_d.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    Ok(EpsSlice {
        axis,
        coord: grid.center(axis, k),
        axes,
        dims: [u.len(), v.len()],
        u,
        v,
        u_nodes: grid.edges(axes[0]).to_vec(),
        v_nodes: grid.edges(axes[1]).to_vec(),
        eps: eps_d,
        medium: med_d,
        media: media.iter().map(|m| m.name.clone()).collect(),
        eps_range: [lo, hi],
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A silicon wire along x in oxide with a dipole and monitors (small, CPU friendly).
    pub fn wire() -> Simulation {
        let j = r#"{
          "name": "wire",
          "region": {"min": [0, -1e-6, -0.6e-6], "max": [3e-6, 1e-6, 0.8e-6]},
          "mesh": {"type": "uniform", "dx": [50e-9, 50e-9, 40e-9]},
          "boundaries": {"x": [{"type": "pml", "layers": 8}, {"type": "pml", "layers": 8}],
                         "y": [{"type": "pml", "layers": 8}, {"type": "pml", "layers": 8}],
                         "z": [{"type": "pml", "layers": 8}, {"type": "pml", "layers": 8}]},
          "run_time": 30e-15,
          "background": "sio2",
          "media": [{"name": "sio2", "eps": [2.085, 2.085, 2.085]}, {"name": "si", "eps": [12.08, 12.08, 12.08]}],
          "structures": [{"name": "wire", "medium": "si", "shape": {"type": "box", "min": [-1, -0.225e-6, 0], "max": [1, 0.225e-6, 0.22e-6]}}],
          "sources": [{"type": "point_dipole", "name": "dipole", "position": [0.6e-6, 0, 0.11e-6], "polarization": "Ey",
                       "signal": {"type": "gaussian_pulse", "freq0": 1.934e14, "fwidth": 4e13}}],
          "monitors": [
            {"type": "flux", "name": "through", "axis": 0, "position": 2.4e-6, "min": [2.4e-6, -0.9e-6, -0.5e-6], "max": [2.4e-6, 0.9e-6, 0.7e-6], "freqs": [1.85e14, 1.934e14, 2.0e14]},
            {"type": "dft_field", "name": "top", "min": [0, -1e-6, 0.11e-6], "max": [3e-6, 1e-6, 0.11e-6], "components": ["Ey"], "freqs": [1.934e14]},
            {"type": "probe", "name": "probe", "position": [1.5e-6, 0, 0.11e-6], "components": ["Ey"]}
          ],
          "shutoff": 1e-4
        }"#;
        Simulation::from_json(j).unwrap()
    }

    #[test]
    fn summary_and_slice() {
        let sim = wire();
        let devs = crate::devices::local_devices();
        let s = summarize("wire", &sim, &devs).unwrap();
        assert_eq!(s.objects.iter().filter(|o| o.kind == "monitor").count(), 3);
        assert!(s.grid.cells > 10_000);
        assert!(s.estimates[0].seconds > 0.0);
        let sl = eps_slice(&sim, 2, 0.11e-6, 4096).unwrap();
        // middle of the wire is silicon, far away oxide
        let iu = sl.u.iter().position(|&x| x > 1.5e-6).unwrap();
        let iv = sl.v.iter().position(|&y| y > 0.0).unwrap();
        assert!((sl.eps[iu * sl.dims[1] + iv] - 12.08).abs() < 0.05);
        assert!((sl.eps[iu * sl.dims[1]] - 2.085).abs() < 0.05);
        let p = preview(&sim, 2.0, 0.5);
        let g = summarize("p", &p, &devs).unwrap();
        assert!(g.grid.cells * 3 < s.grid.cells);
    }
}

/// |E|² of a mode profile stored in the spec (a mode source or monitor), on
/// the plane's grid cells.
#[derive(Clone, Debug, Serialize)]
pub struct ModeView {
    pub name: String,
    pub neff: f64,
    pub neff_imag: f64,
    pub freq: f64,
    pub axis: usize,
    pub axes: [usize; 2],
    pub dims: [usize; 2],
    pub u: Vec<f64>,
    pub v: Vec<f64>,
    pub intensity: Vec<f32>,
}

pub fn mode_profile(sim: &Simulation, name: &str, index: usize) -> Result<ModeView> {
    use lightsim_core::monitors::Monitor as M;
    use lightsim_core::sources::Source as S;
    let found = sim
        .sources
        .iter()
        .find_map(|s| match s {
            S::Mode {
                name: n,
                axis,
                min,
                max,
                profile,
                ..
            } if n == name => Some((*axis, *min, *max, profile.clone())),
            _ => None,
        })
        .or_else(|| {
            sim.monitors.iter().find_map(|m| match m {
                M::Mode {
                    name: n,
                    axis,
                    min,
                    max,
                    modes,
                    ..
                } if n == name => modes.get(index).map(|p| (*axis, *min, *max, p.clone())),
                _ => None,
            })
        });
    let Some((axis, min, max, p)) = found else {
        anyhow::bail!("no mode source or mode monitor named '{name}' with a mode {index}")
    };
    let grid = sim.build_grid()?;
    let axes = match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    let [nu, nv] = p.dims;
    let coords = |a: usize, n: usize| -> Vec<f64> {
        let i0 = grid.cell_index(a, min[a]);
        (0..n)
            .map(|i| grid.center(a, (i0 + i).min(grid.n()[a] - 1)))
            .collect()
    };
    let mut intensity = vec![0f32; nu * nv];
    for c in 0..2 {
        for (k, x) in intensity.iter_mut().enumerate() {
            let re = p.e_re[c].get(k).copied().unwrap_or(0.0);
            let im = p.e_im[c].get(k).copied().unwrap_or(0.0);
            *x += (re * re + im * im) as f32;
        }
    }
    let _ = max;
    Ok(ModeView {
        name: name.into(),
        neff: p.neff,
        neff_imag: p.neff_imag,
        freq: p.freq,
        axis,
        axes,
        dims: [nu, nv],
        u: coords(axes[0], nu),
        v: coords(axes[1], nv),
        intensity,
    })
}
