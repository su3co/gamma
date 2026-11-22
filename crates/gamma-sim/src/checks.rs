//! Consistency checks of a simulation file against itself and the layout.

use gamma_core::checks::{Finding, Severity};
use gamma_core::layout::Layout;
use gamma_core::stack::Stack;
use lightsim_core::Simulation;
use lightsim_core::geometry::Shape;

pub fn check(
    name: &str,
    sim: &Simulation,
    layout: Option<&Layout>,
    stack: Option<&Stack>,
) -> Vec<Finding> {
    let mut out = vec![];
    let obj = format!("sim:{name}");
    if let Err(e) = sim.validate() {
        out.push(
            Finding::new(
                Severity::Error,
                "sim.invalid",
                format!("simulation {name}: {e:#}"),
            )
            .on(&obj),
        );
        return out;
    }
    let media = match sim.media_table() {
        Ok(m) => m,
        Err(e) => {
            out.push(
                Finding::new(
                    Severity::Error,
                    "sim.media",
                    format!("simulation {name}: {e:#}"),
                )
                .on(&obj),
            );
            return out;
        }
    };
    let grid = match sim.build_grid() {
        Ok(g) => g,
        Err(e) => {
            out.push(
                Finding::new(
                    Severity::Error,
                    "sim.grid",
                    format!("simulation {name}: {e:#}"),
                )
                .on(&obj),
            );
            return out;
        }
    };
    let c = lightsim_core::constants::C0;
    let fmax = sim
        .sources
        .iter()
        .filter_map(|s| s.signal().freq0())
        .fold(0.0, f64::max);
    if fmax > 0.0 {
        let nmax = media
            .iter()
            .map(|m| m.eps.iter().cloned().fold(1.0, f64::max).sqrt())
            .fold(1.0, f64::max);
        let nbg = media
            .iter()
            .find(|m| m.name == sim.background)
            .map(|m| m.eps.iter().cloned().fold(1.0, f64::max).sqrt())
            .unwrap_or(1.0);
        let lam0 = c / fmax;
        let n = grid.n();
        for a in 0..3 {
            if sim.invariant() == Some(a) {
                continue;
            }
            let dmin = (0..n[a])
                .map(|i| grid.cell_size(a, i))
                .fold(f64::INFINITY, f64::min);
            let dmax = (0..n[a]).map(|i| grid.cell_size(a, i)).fold(0.0, f64::max);
            // the finest cells sit in the densest medium (graded meshes); the coarsest in the background
            let ppw = (lam0 / (nmax * dmin)).min(lam0 / (nbg * dmax));
            if ppw < 8.0 {
                out.push(Finding::new(Severity::Warning, "sim.mesh_resolution", format!("simulation {name}: about {ppw:.1} cells per wavelength along {} (n = {nmax:.2} in the densest medium); results may be inaccurate below 10-15", ["x", "y", "z"][a])).on(&obj));
            }
        }
        for a in 0..3 {
            for (side, b) in sim.boundaries.axis(a).iter().enumerate() {
                let layers = b.pml_layers();
                if layers == 0 {
                    continue;
                }
                let i = if side == 0 { 0 } else { n[a] - 1 };
                let thick = layers as f64 * grid.cell_size(a, i);
                if thick < 0.25 * c / fmax {
                    out.push(Finding::new(Severity::Info, "sim.pml_thin", format!("simulation {name}: the absorbing layer on the {} {} side is {:.2} um, less than a quarter of a free-space wavelength", if side == 0 { "low" } else { "high" }, ["x", "y", "z"][a], thick * 1e6)).on(&obj));
                }
            }
        }
    } else if sim.sources.is_empty() {
        out.push(
            Finding::new(
                Severity::Error,
                "sim.no_source",
                format!("simulation {name} has no source"),
            )
            .on(&obj),
        );
    }
    if sim.monitors.is_empty() {
        out.push(
            Finding::new(
                Severity::Warning,
                "sim.no_monitor",
                format!("simulation {name} has no monitor: the run would record nothing"),
            )
            .on(&obj),
        );
    }
    if sim.shutoff.is_none() {
        out.push(Finding::new(Severity::Info, "sim.no_shutoff", format!("simulation {name} has no shut-off: it runs for the full run time even after the fields have decayed")).on(&obj));
    }
    // sources and monitors inside the region
    let inside = |p: &[f64; 3]| {
        (0..3).all(|a| p[a] >= sim.region.min[a] - 1e-12 && p[a] <= sim.region.max[a] + 1e-12)
    };
    let v = serde_json::to_value(sim).unwrap_or_default();
    for (kind, list) in [("source", &v["sources"]), ("monitor", &v["monitors"])] {
        for o in list.as_array().cloned().unwrap_or_default() {
            for key in ["position", "center", "min", "max"] {
                if let Some(a) = o[key].as_array()
                    && a.len() == 3
                {
                    let p = [
                        a[0].as_f64().unwrap_or(0.0),
                        a[1].as_f64().unwrap_or(0.0),
                        a[2].as_f64().unwrap_or(0.0),
                    ];
                    if !inside(&p) {
                        let nm = o["name"].as_str().unwrap_or("?");
                        out.push(Finding::new(Severity::Warning, "sim.outside_region", format!("simulation {name}: {kind} '{nm}' has its {key} outside the simulation region")).on(format!("{kind}:{name}/{nm}")));
                        break;
                    }
                }
            }
        }
    }
    // layout coverage: polygons of the stack's patterned layers should appear as poly slabs
    if let (Some(layout), Some(stack)) = (layout, stack) {
        let slabs: Vec<([f64; 3], [f64; 3])> = sim
            .structures
            .iter()
            .filter(|s| matches!(s.shape, Shape::PolySlab { axis: 2, .. }))
            .map(|s| s.shape.bbox())
            .collect();
        if !slabs.is_empty() {
            for l in &layout.layers {
                let Some(sl) = stack.layer_for(l.key.0, l.key.1) else {
                    continue;
                };
                let mut missing = 0usize;
                let rx = [sim.region.min[0] * 1e6, sim.region.max[0] * 1e6];
                let ry = [sim.region.min[1] * 1e6, sim.region.max[1] * 1e6];
                for b in &l.bboxes {
                    let cx = 0.5 * (b.min[0] + b.max[0]);
                    let cy = 0.5 * (b.min[1] + b.max[1]);
                    if cx < rx[0] || cx > rx[1] || cy < ry[0] || cy > ry[1] {
                        continue; // outside the simulated window
                    }
                    let covered = slabs.iter().any(|(mn, mx)| {
                        cx * 1e-6 >= mn[0] - 1e-9
                            && cx * 1e-6 <= mx[0] + 1e-9
                            && cy * 1e-6 >= mn[1] - 1e-9
                            && cy * 1e-6 <= mx[1] + 1e-9
                    });
                    if !covered {
                        missing += 1;
                    }
                }
                if missing > 0 {
                    out.push(Finding::new(Severity::Warning, "sim.layout_mismatch", format!("simulation {name}: {missing} polygon(s) of layer {} inside the simulated window have no matching structure; the spec may be older than the layout", sl.name)).on(&obj));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wire_checks() {
        let sim = crate::setup::tests::wire();
        let f = check("wire", &sim, None, None);
        assert!(!f.iter().any(|f| f.severity == Severity::Error), "{f:?}");
    }
}
