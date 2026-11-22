//! Reduction of lightsim results (the JSON `lightsim run --out` writes) to
//! what the results view and the tools show: spectra, S-parameters, field
//! maps, time series, far fields and the energy decay.

use anyhow::{Context, Result, bail};
use lightsim_core::constants::C0;
use lightsim_core::grid::Component;
use lightsim_core::Simulation;
use num_complex::Complex64;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::Path;

pub struct Loaded {
    pub results: Value,
    pub sim: Simulation,
}

impl Loaded {
    pub fn read(results: &Path, spec: &Simulation) -> Result<Loaded> {
        let text = std::fs::read_to_string(results).with_context(|| format!("reading {}", results.display()))?;
        let v: Value = serde_json::from_str(&text).with_context(|| format!("{} is not a lightsim results file", results.display()))?;
        Ok(Loaded { results: v, sim: spec.clone() })
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct MonitorView {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    /// Views the results view can draw for this monitor.
    pub views: Vec<&'static str>,
    pub freqs: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Overview {
    pub stats: Value,
    pub monitors: Vec<MonitorView>,
    pub sparams: Option<SParams>,
    pub metrics: Value,
}


fn f64s(v: &Value) -> Vec<f64> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).collect()).unwrap_or_default()
}

pub fn overview(l: &Loaded) -> Result<Overview> {
    let sj = serde_json::to_value(&l.sim)?;
    let r = &l.results;
    let mut monitors = vec![];
    for m in sj["monitors"].as_array().cloned().unwrap_or_default() {
        let name = m["name"].as_str().unwrap_or("").to_string();
        let ty = m["type"].as_str().unwrap_or("").to_string();
        let nf = m["freqs"].as_array().map(|a| a.len()).unwrap_or(0);
        let views: Vec<&'static str> = match ty.as_str() {
            "flux" | "flux_box" => vec!["spectrum"],
            "mode" => vec!["spectrum", "mode_amplitudes"],
            "dft_field" => vec!["field_map"],
            "far_field" => vec!["far_field", "spectrum"],
            "probe" | "field_time" => vec!["time_series"],
            "energy" => vec!["time_series"],
            _ => vec![],
        };
        monitors.push(MonitorView { name, ty, views, freqs: nf });
    }
    let sp = sparams(l).ok().flatten();
    let mut stats = r["stats"].clone();
    if let Some(o) = stats.as_object_mut() {
        // keep the energy trace out of the overview (served by energy_decay)
        o.remove("energy");
        o.remove("energy_steps");
    }
    Ok(Overview { stats, monitors, metrics: metrics(l, sp.as_ref()), sparams: sp })
}

/// Scalar figures a result diff compares.
pub fn metrics(l: &Loaded, sp: Option<&SParams>) -> Value {
    let mut m = serde_json::Map::new();
    let r = &l.results;
    if let Some(s) = r["stats"].as_object() {
        for k in ["steps", "seconds", "mcells_per_s", "cells"] {
            if let Some(v) = s.get(k) {
                m.insert(format!("run.{k}"), v.clone());
            }
        }
    }
    if let Some(sp) = sp {
        let mid = sp.wavelengths.len() / 2;
        for e in &sp.entries {
            if let Some(db) = e.db.get(mid) {
                m.insert(format!("S.{}.dB@{:.1}nm", e.name, sp.wavelengths[mid] * 1e9), json!(db));
            }
        }
    }
    if let Some(fl) = r["flux"].as_object() {
        for (name, v) in fl {
            let a = f64s(v);
            if !a.is_empty() {
                m.insert(format!("flux.{name}.max"), json!(a.iter().cloned().fold(f64::NEG_INFINITY, f64::max)));
            }
        }
    }
    Value::Object(m)
}

#[derive(Clone, Debug, Serialize)]
pub struct Spectrum {
    pub monitor: String,
    pub wavelengths: Vec<f64>,
    pub series: Vec<Series>,
    pub unit: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Series {
    pub name: String,
    pub values: Vec<f64>,
}

fn monitor_freqs(sj: &Value, name: &str) -> Vec<f64> {
    sj["monitors"].as_array().and_then(|a| a.iter().find(|m| m["name"] == name)).map(|m| f64s(&m["freqs"])).unwrap_or_default()
}

pub fn spectrum(l: &Loaded, monitor: &str) -> Result<Spectrum> {
    let sj = serde_json::to_value(&l.sim)?;
    let freqs = monitor_freqs(&sj, monitor);
    let r = &l.results;
    let mut series = vec![];
    if let Some(m) = r["modes"].get(monitor) {
        let ap = mode_amps(m, "plus");
        let am = mode_amps(m, "minus");
        for (k, (p, q)) in ap.iter().zip(&am).enumerate() {
            series.push(Series { name: format!("mode {k} +"), values: p.iter().map(|c| c.norm_sqr()).collect() });
            series.push(Series { name: format!("mode {k} -"), values: q.iter().map(|c| c.norm_sqr()).collect() });
        }
    }
    if let Some(f) = r["flux"].get(monitor) {
        series.push(Series { name: "flux".into(), values: f64s(f) });
    }
    if series.is_empty() {
        bail!("monitor '{monitor}' has no spectrum in these results");
    }
    let mut wl: Vec<f64> = freqs.iter().map(|f| C0 / f).collect();
    // ascending wavelength
    let mut order: Vec<usize> = (0..wl.len()).collect();
    order.sort_by(|&a, &b| wl[a].partial_cmp(&wl[b]).unwrap());
    wl = order.iter().map(|&i| wl[i]).collect();
    for s in series.iter_mut() {
        if s.values.len() == order.len() {
            s.values = order.iter().map(|&i| s.values[i]).collect();
        }
    }
    Ok(Spectrum { monitor: monitor.into(), wavelengths: wl, series, unit: "W (source-weighted, arbitrary scale)".into() })
}

fn mode_amps(m: &Value, dir: &str) -> Vec<Vec<Complex64>> {
    let re = &m[format!("amp_{dir}_re")];
    let im = &m[format!("amp_{dir}_im")];
    let (Some(re), Some(im)) = (re.as_array(), im.as_array()) else { return vec![] };
    re.iter().zip(im).map(|(a, b)| f64s(a).into_iter().zip(f64s(b)).map(|(x, y)| Complex64::new(x, y)).collect()).collect()
}

#[derive(Clone, Debug, Serialize)]
pub struct SEntry {
    /// `o2@0,o1@0` (gdsfactory convention: output, input).
    pub name: String,
    pub db: Vec<f64>,
    pub phase: Vec<f64>,
    pub abs2: Vec<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SParams {
    pub input: String,
    pub wavelengths: Vec<f64>,
    pub entries: Vec<SEntry>,
    pub note: String,
}

/// S-parameters of a run with one mode source and mode monitors, as
/// lightsim's `write_sparameters` computes them: the amplitude of the mode
/// leaving the device at each monitor over the incident amplitude at the
/// monitor next to the source.
pub fn sparams(l: &Loaded) -> Result<Option<SParams>> {
    use lightsim_core::monitors::Monitor as M;
    use lightsim_core::sources::Source as S;
    let srcs: Vec<&S> = l.sim.sources.iter().filter(|s| matches!(s, S::Mode { .. })).collect();
    if srcs.len() != 1 || l.sim.sources.len() != 1 {
        return Ok(None);
    }
    let S::Mode { axis: sa, position: sp, direction, name: sname, .. } = srcs[0] else { return Ok(None) };
    let sign = direction.sign();
    let mons: Vec<(&str, usize, f64, &Vec<f64>)> = l
        .sim
        .monitors
        .iter()
        .filter_map(|m| match m {
            M::Mode { name, axis, position, freqs, .. } => Some((name.as_str(), *axis, *position, freqs)),
            _ => None,
        })
        .collect();
    if mons.is_empty() {
        return Ok(None);
    }
    // input monitor: same axis, downstream of the source, nearest
    let input = mons
        .iter()
        .filter(|m| m.1 == *sa && (m.2 - sp) * sign >= 0.0)
        .min_by(|a, b| ((a.2 - sp).abs()).partial_cmp(&(b.2 - sp).abs()).unwrap());
    let Some(input) = input else { return Ok(None) };
    let r = &l.results;
    let amp = |name: &str, plus: bool| -> Option<Vec<Complex64>> { mode_amps(r["modes"].get(name)?, if plus { "plus" } else { "minus" }).into_iter().next() };
    let a_in = amp(input.0, sign > 0.0).context("input monitor has no amplitudes")?;
    let center: [f64; 3] = [0, 1, 2].map(|a| 0.5 * (l.sim.region.min[a] + l.sim.region.max[a]));
    let port = |n: &str| n.strip_prefix("m_").unwrap_or(n).to_string();
    let in_port = port(sname.strip_prefix("src_").unwrap_or(input.0));
    let mut entries = vec![];
    for m in &mons {
        let out = if m.0 == input.0 {
            amp(m.0, sign < 0.0) // reflection travels against the source
        } else {
            amp(m.0, m.2 > center[m.1]) // leaving the device, away from its centre
        };
        let Some(out) = out else { continue };
        let s: Vec<Complex64> = out.iter().zip(&a_in).map(|(o, i)| if i.norm() > 0.0 { o / i } else { Complex64::new(0.0, 0.0) }).collect();
        entries.push(SEntry {
            name: format!("{}@0,{}@0", port(m.0), in_port),
            db: s.iter().map(|c| 10.0 * c.norm_sqr().max(1e-30).log10()).collect(),
            phase: s.iter().map(|c| c.arg()).collect(),
            abs2: s.iter().map(|c| c.norm_sqr()).collect(),
        });
    }
    let wl: Vec<f64> = input.3.iter().map(|f| C0 / f).collect();
    Ok(Some(SParams {
        input: in_port,
        wavelengths: wl,
        entries,
        note: "S = (mode amplitude leaving the device at each monitor) / (incident amplitude at the monitor next to the source), fundamental mode".into(),
    }))
}

#[derive(Clone, Debug, Serialize)]
pub struct FieldMap {
    pub monitor: String,
    pub quantity: String,
    pub freq: f64,
    pub wavelength: f64,
    pub axis: usize,
    pub axes: [usize; 2],
    pub dims: [usize; 2],
    pub u: Vec<f64>,
    pub v: Vec<f64>,
    pub data: Vec<f32>,
    pub range: [f32; 2],
    pub freqs: Vec<f64>,
}

/// A plane of a frequency-domain monitor: `|E|^2`, `|H|^2`, or one component's
/// real part, magnitude or phase (`Ey`, `abs(Ey)`, `phase(Ey)`).
pub fn field_map(l: &Loaded, monitor: &str, quantity: &str, freq_index: usize) -> Result<FieldMap> {
    let d = l.results["dft"].get(monitor).with_context(|| format!("no frequency-domain monitor '{monitor}' in these results"))?;
    let freqs = f64s(&d["freqs"]);
    let fields = d["fields"].as_array().context("monitor has no fields")?;
    let grid = l.sim.build_grid()?;
    let fi = freq_index.min(freqs.len().saturating_sub(1));
    let comp_names: Vec<String> = fields.iter().filter_map(|f| f["comp"].as_str().map(|s| s.to_string())).collect();
    let (wanted, mode): (Vec<String>, &str) = if quantity == "|E|2" || quantity == "|E|^2" {
        (comp_names.iter().filter(|c| c.starts_with('E')).cloned().collect(), "sum2")
    } else if quantity == "|H|2" || quantity == "|H|^2" {
        (comp_names.iter().filter(|c| c.starts_with('H')).cloned().collect(), "sum2")
    } else if let Some(c) = quantity.strip_prefix("abs(").and_then(|s| s.strip_suffix(')')) {
        (vec![c.to_string()], "abs")
    } else if let Some(c) = quantity.strip_prefix("phase(").and_then(|s| s.strip_suffix(')')) {
        (vec![c.to_string()], "phase")
    } else {
        (vec![quantity.to_string()], "re")
    };
    if wanted.is_empty() {
        bail!("monitor '{monitor}' records {}; '{quantity}' needs other components", comp_names.join(", "));
    }
    // common box of the wanted components (Yee components differ by a sample)
    let mut entries = vec![];
    for w in &wanted {
        let f = fields.iter().find(|f| f["comp"] == w.as_str()).with_context(|| format!("monitor '{monitor}' did not record {w}; it records {}", comp_names.join(", ")))?;
        let origin: Vec<usize> = f["origin"].as_array().context("origin")?.iter().map(|x| x.as_u64().unwrap_or(0) as usize).collect();
        let dims: Vec<usize> = f["dims"].as_array().context("dims")?.iter().map(|x| x.as_u64().unwrap_or(0) as usize).collect();
        entries.push((w.clone(), origin, dims, f64s(&f["re"]), f64s(&f["im"])));
    }
    let dims0 = &entries[0].2;
    let normal = (1..4).find(|&a| dims0[a] == 1).map(|a| a - 1).unwrap_or(2);
    let axes = match normal {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    let nu = entries.iter().map(|e| e.2[axes[0] + 1]).min().unwrap();
    let nv = entries.iter().map(|e| e.2[axes[1] + 1]).min().unwrap();
    let mut data = vec![0f32; nu * nv];
    for (name, _origin, dims, re, im) in &entries {
        let (nx, ny, nz) = (dims[1], dims[2], dims[3]);
        for iu in 0..nu {
            for iv in 0..nv {
                let mut ijk = [0usize; 3];
                ijk[axes[0]] = iu;
                ijk[axes[1]] = iv;
                let idx = ((fi * nx + ijk[0]) * ny + ijk[1]) * nz + ijk[2];
                let c = Complex64::new(re.get(idx).copied().unwrap_or(0.0), im.get(idx).copied().unwrap_or(0.0));
                let val = match mode {
                    "sum2" => c.norm_sqr(),
                    "abs" => c.norm(),
                    "phase" => c.arg(),
                    _ => c.re,
                };
                data[iu * nv + iv] += val as f32;
                let _ = name;
            }
        }
    }
    let comp0 = Component::parse(&entries[0].0).unwrap_or(Component::Ex);
    let o = &entries[0].1;
    let u: Vec<f64> = (0..nu).map(|i| grid.position(comp0, axes[0], o[axes[0]] + i)).collect();
    let v: Vec<f64> = (0..nv).map(|i| grid.position(comp0, axes[1], o[axes[1]] + i)).collect();
    let lo = data.iter().cloned().fold(f32::INFINITY, f32::min);
    let hi = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    Ok(FieldMap {
        monitor: monitor.into(),
        quantity: quantity.into(),
        freq: freqs.get(fi).copied().unwrap_or(0.0),
        wavelength: freqs.get(fi).map(|f| C0 / f).unwrap_or(0.0),
        axis: normal,
        axes,
        dims: [nu, nv],
        u,
        v,
        data,
        range: [lo, hi],
        freqs,
    })
}

#[derive(Clone, Debug, Serialize)]
pub struct TimeSeries {
    pub monitor: String,
    pub times: Vec<f64>,
    pub series: Vec<Series>,
}

pub fn time_series(l: &Loaded, monitor: &str) -> Result<TimeSeries> {
    let r = &l.results;
    if let Some(t) = r["time"].get(monitor) {
        let times = f64s(&t["times"]);
        let mut series = vec![];
        for f in t["fields"].as_array().cloned().unwrap_or_default() {
            let data = f64s(&f["data"]);
            let dims: Vec<usize> = f["dims"].as_array().map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0) as usize).collect()).unwrap_or_default();
            let per = dims.get(1..).map(|d| d.iter().product::<usize>()).unwrap_or(1).max(1);
            // a probe: one sample per time; a box: its centre sample
            let mid = per / 2;
            let vals: Vec<f64> = (0..times.len()).map(|i| data.get(i * per + mid).copied().unwrap_or(0.0)).collect();
            series.push(Series { name: f["comp"].as_str().unwrap_or("").into(), values: vals });
        }
        return Ok(TimeSeries { monitor: monitor.into(), times, series });
    }
    if let Some(e) = r["energy"].get(monitor) {
        return Ok(TimeSeries { monitor: monitor.into(), times: f64s(&e["times"]), series: vec![Series { name: "energy (J)".into(), values: f64s(&e["energy"]) }] });
    }
    bail!("monitor '{monitor}' has no time series in these results")
}

/// Field energy over its peak against time: the decay the shut-off compares.
pub fn energy_decay(l: &Loaded) -> TimeSeries {
    let s = &l.results["stats"];
    let steps = f64s(&s["energy_steps"]);
    let e = f64s(&s["energy"]);
    let dt = s["dt"].as_f64().unwrap_or(0.0);
    let peak = e.iter().cloned().fold(0.0, f64::max).max(1e-300);
    TimeSeries { monitor: "energy decay".into(), times: steps.iter().map(|n| n * dt).collect(), series: vec![Series { name: "energy / peak".into(), values: e.iter().map(|x| x / peak).collect() }] }
}

#[derive(Clone, Debug, Serialize)]
pub struct FarField {
    pub monitor: String,
    pub freq: f64,
    pub theta: Vec<f64>,
    pub phi: Vec<f64>,
    /// Intensity [theta][phi], W/sr.
    pub intensity: Vec<f64>,
}

pub fn far_field(l: &Loaded, monitor: &str, freq_index: usize) -> Result<FarField> {
    let f = l.results["far_field"].get(monitor).with_context(|| format!("no far-field monitor '{monitor}'"))?;
    let freqs = f64s(&f["freqs"]);
    let theta = f64s(&f["theta"]);
    let phi = f64s(&f["phi"]);
    let all = f64s(&f["intensity"]);
    let fi = freq_index.min(freqs.len().saturating_sub(1));
    let n = theta.len() * phi.len();
    Ok(FarField { monitor: monitor.into(), freq: freqs.get(fi).copied().unwrap_or(0.0), intensity: all.get(fi * n..(fi + 1) * n).map(|s| s.to_vec()).unwrap_or_default(), theta, phi })
}

/// Compare the metrics of two runs.
pub fn diff_metrics(a: &Value, b: &Value) -> Value {
    let mut out = vec![];
    let (Some(a), Some(b)) = (a.as_object(), b.as_object()) else { return json!([]) };
    let mut keys: Vec<&String> = a.keys().chain(b.keys()).collect();
    keys.sort();
    keys.dedup();
    for k in keys {
        let x = a.get(k).and_then(|v| v.as_f64());
        let y = b.get(k).and_then(|v| v.as_f64());
        out.push(json!({"metric": k, "before": x, "after": y, "change": match (x, y) { (Some(x), Some(y)) => Some(y - x), _ => None }}));
    }
    Value::Array(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce_wire() {
        let sim = crate::setup::tests::wire();
        let res = lightsim_core::run_cpu(&sim).unwrap();
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("r.json");
        std::fs::write(&p, res.to_json()).unwrap();
        let l = Loaded::read(&p, &sim).unwrap();
        let o = overview(&l).unwrap();
        assert_eq!(o.monitors.len(), 3);
        let s = spectrum(&l, "through").unwrap();
        assert_eq!(s.wavelengths.len(), 3);
        assert!(s.wavelengths[0] < s.wavelengths[2]);
        let f = field_map(&l, "top", "|E|2", 0).unwrap();
        assert_eq!(f.axis, 2);
        assert!(f.range[1] > 0.0);
        let f = field_map(&l, "top", "Ey", 0).unwrap();
        assert!(f.range[0] < 0.0 || f.range[1] > 0.0);
        let t = time_series(&l, "probe").unwrap();
        assert_eq!(t.series[0].values.len(), t.times.len());
        let e = energy_decay(&l);
        assert!(!e.times.is_empty());
        assert!(sparams(&l).unwrap().is_none());
    }
}
