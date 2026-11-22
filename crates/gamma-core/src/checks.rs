//! Design-rule and project consistency checks.
//!
//! The width and spacing checks rasterise each layer on a grid of an eighth
//! of the rule (at most 16 million cells per layer; coarser for very large
//! layers, which the report states) and compare the layer with its
//! morphological opening (width) or closing (spacing) by a disc of the rule's
//! diameter, computed with exact Euclidean distance transforms. Clusters the
//! size of the rounding a disc leaves at a sharp corner are not reported.

use crate::geom::BBox;
use crate::layout::Layout;
use crate::project::Project;
use crate::stack::Stack;
use serde::Serialize;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Clone, Debug, Serialize)]
pub struct Finding {
    pub severity: Severity,
    /// Rule identifier (`drc.min_width`, `sim.mesh_resolution`, ...).
    pub rule: String,
    pub message: String,
    /// Object reference the finding is about (`layer:1/0`, `sim:ringdown`, ...).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
}

impl Finding {
    pub fn new(severity: Severity, rule: &str, message: impl Into<String>) -> Finding {
        Finding {
            severity,
            rule: rule.into(),
            message: message.into(),
            object: None,
            bbox: None,
        }
    }
    pub fn on(mut self, object: impl Into<String>) -> Finding {
        self.object = Some(object.into());
        self
    }
    pub fn at(mut self, b: BBox) -> Finding {
        self.bbox = Some(b);
        self
    }
}

const MAX_CELLS: usize = 16_000_000;

struct Raster {
    nx: usize,
    ny: usize,
    x0: f64,
    y0: f64,
    r: f64,
    inside: Vec<bool>,
}

fn rasterize(polys: &[Vec<[f64; 2]>], b: &BBox, r: f64) -> Raster {
    let nx = (b.width() / r).ceil() as usize + 1;
    let ny = (b.height() / r).ceil() as usize + 1;
    let mut inside = vec![false; nx * ny];
    let mut xs = vec![];
    for p in polys {
        let pb = BBox::of(p);
        let j0 = (((pb.min[1] - b.min[1]) / r).floor().max(0.0)) as usize;
        let j1 = ((((pb.max[1] - b.min[1]) / r).ceil()) as usize).min(ny);
        for j in j0..j1 {
            let y = b.min[1] + (j as f64 + 0.5) * r;
            xs.clear();
            let n = p.len();
            for i in 0..n {
                let (a, c) = (p[i], p[(i + 1) % n]);
                if (a[1] > y) != (c[1] > y) {
                    xs.push(a[0] + (y - a[1]) * (c[0] - a[0]) / (c[1] - a[1]));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            for pair in xs.as_chunks::<2>().0 {
                let i0 = ((pair[0] - b.min[0]) / r - 0.5).ceil().max(0.0) as usize;
                let i1 = (((pair[1] - b.min[0]) / r - 0.5).floor() as isize).min(nx as isize - 1);
                if i1 < i0 as isize {
                    continue;
                }
                for i in i0..=(i1 as usize) {
                    // union of polygons (overlapping shapes on one layer merge)
                    inside[j * nx + i] = true;
                }
            }
        }
    }
    Raster {
        nx,
        ny,
        x0: b.min[0],
        y0: b.min[1],
        r,
        inside,
    }
}

/// Squared Euclidean distance (in cells) from every cell to the nearest cell where `seed` is true.
fn edt(seed: &[bool], nx: usize, ny: usize) -> Vec<f64> {
    const INF: f64 = 1e20;
    let mut f = vec![0.0; nx * ny];
    for i in 0..nx * ny {
        f[i] = if seed[i] { 0.0 } else { INF };
    }
    let dt1 = |src: &[f64], out: &mut [f64], n: usize| {
        let mut v = vec![0usize; n];
        let mut z = vec![0.0f64; n + 1];
        let mut k = 0usize;
        v[0] = 0;
        z[0] = -INF;
        z[1] = INF;
        for q in 1..n {
            loop {
                let p = v[k];
                let s = ((src[q] + (q * q) as f64) - (src[p] + (p * p) as f64))
                    / (2.0 * q as f64 - 2.0 * p as f64);
                if s <= z[k] {
                    if k == 0 {
                        v[0] = q;
                        z[0] = -INF;
                        z[1] = INF;
                        break;
                    }
                    k -= 1;
                    continue;
                }
                k += 1;
                v[k] = q;
                z[k] = s;
                z[k + 1] = INF;
                break;
            }
        }
        k = 0;
        for q in 0..n {
            while z[k + 1] < q as f64 {
                k += 1;
            }
            let d = q as f64 - v[k] as f64;
            out[q] = d * d + src[v[k]];
        }
    };
    // columns
    let mut col = vec![0.0; ny];
    let mut outc = vec![0.0; ny];
    for i in 0..nx {
        for j in 0..ny {
            col[j] = f[j * nx + i];
        }
        dt1(&col, &mut outc, ny);
        for j in 0..ny {
            f[j * nx + i] = outc[j];
        }
    }
    let mut row = vec![0.0; nx];
    for j in 0..ny {
        row.copy_from_slice(&f[j * nx..(j + 1) * nx]);
        dt1(&row, &mut f[j * nx..(j + 1) * nx], nx);
    }
    f
}

/// Connected clusters of flagged cells: (cells, bbox in cells, max of `depth` over the cluster).
fn clusters(flag: &[bool], nx: usize, ny: usize, depth: &[f64]) -> Vec<(usize, [usize; 4], f64)> {
    let mut seen = vec![false; flag.len()];
    let mut out = vec![];
    let mut stack = vec![];
    for s in 0..flag.len() {
        if !flag[s] || seen[s] {
            continue;
        }
        seen[s] = true;
        stack.push(s);
        let mut n = 0;
        let mut bb = [usize::MAX, usize::MAX, 0, 0];
        let mut dmax: f64 = 0.0;
        while let Some(c) = stack.pop() {
            n += 1;
            let (i, j) = (c % nx, c / nx);
            bb = [bb[0].min(i), bb[1].min(j), bb[2].max(i), bb[3].max(j)];
            dmax = dmax.max(depth[c]);
            let mut push = |ii: usize, jj: usize| {
                let k = jj * nx + ii;
                if flag[k] && !seen[k] {
                    seen[k] = true;
                    stack.push(k);
                }
            };
            if i > 0 {
                push(i - 1, j)
            }
            if i + 1 < nx {
                push(i + 1, j)
            }
            if j > 0 {
                push(i, j - 1)
            }
            if j + 1 < ny {
                push(i, j + 1)
            }
        }
        out.push((n, bb, dmax));
    }
    out
}

/// Width and spacing violations on one layer.
pub fn drc_layer(
    polys: &[Vec<[f64; 2]>],
    layer_ref: &str,
    layer_name: &str,
    min_width: Option<f64>,
    min_space: Option<f64>,
) -> Vec<Finding> {
    let mut out = vec![];
    let mut lb = BBox::EMPTY;
    for p in polys {
        lb.union(&BBox::of(p));
    }
    if lb.is_empty() {
        return out;
    }
    for (rule, value) in [("drc.min_width", min_width), ("drc.min_space", min_space)] {
        let Some(w) = value.filter(|w| *w > 0.0) else {
            continue;
        };
        let b = lb.grow(w * 1.5);
        let mut r = w / 8.0;
        let cells = (b.width() / r) * (b.height() / r);
        let mut note = String::new();
        if cells > MAX_CELLS as f64 {
            r *= (cells / MAX_CELLS as f64).sqrt();
            note = format!(
                " (checked on a {:.0} nm grid: layer too large for the {:.0} nm default)",
                r * 1e3,
                w / 8.0 * 1e3
            );
        }
        if r > w / 3.0 {
            out.push(Finding::new(Severity::Warning, rule, format!("layer {layer_name} is too large to check {rule} at {w} um on one grid; skipped")).on(layer_ref));
            continue;
        }
        let ras = rasterize(polys, &b, r);
        let (nx, ny) = (ras.nx, ras.ny);
        let h = w / 2.0 / r; // rule radius in cells
        // width: opening of the inside; spacing: closing (= opening of the outside)
        let solid: Vec<bool> = if rule == "drc.min_width" {
            ras.inside.clone()
        } else {
            ras.inside.iter().map(|v| !v).collect()
        };
        let not_solid: Vec<bool> = solid.iter().map(|v| !v).collect();
        let d_in = edt(&not_solid, nx, ny); // distance from solid cells to the outside
        let core: Vec<bool> = d_in.iter().map(|d| d.sqrt() >= h).collect();
        let d_core = edt(&core, nx, ny);
        let flagged: Vec<bool> = (0..nx * ny)
            .map(|i| solid[i] && d_core[i].sqrt() > h + 0.75)
            .collect();
        let depth: Vec<f64> = d_core.iter().map(|d| d.sqrt()).collect();
        for (n, bb, dmax) in clusters(&flagged, nx, ny, &depth) {
            let area = n as f64; // cells
            let corner = 0.25 * h * h; // rounding a disc leaves in a sharp corner
            if dmax < 1.5 * h && area < 2.5 * corner {
                continue;
            }
            let bbox = BBox {
                min: [ras.x0 + bb[0] as f64 * ras.r, ras.y0 + bb[1] as f64 * ras.r],
                max: [
                    ras.x0 + (bb[2] + 1) as f64 * ras.r,
                    ras.y0 + (bb[3] + 1) as f64 * ras.r,
                ],
            };
            // spacing clusters touching the padded edge are open space, not a gap
            if rule == "drc.min_space"
                && (bb[0] == 0 || bb[1] == 0 || bb[2] + 1 >= nx || bb[3] + 1 >= ny)
            {
                continue;
            }
            let what = if rule == "drc.min_width" {
                "narrower than the minimum width"
            } else {
                "a gap narrower than the minimum spacing"
            };
            out.push(
                Finding::new(
                    Severity::Error,
                    rule,
                    format!("layer {layer_name}: {what} {w} um near ({:.3}, {:.3}) um, over {:.3} x {:.3} um{note}", 0.5 * (bbox.min[0] + bbox.max[0]), 0.5 * (bbox.min[1] + bbox.max[1]), bbox.width(), bbox.height()),
                )
                .on(layer_ref)
                .at(bbox),
            );
        }
    }
    out
}

/// Design-rule check of every layer that has rules in the stack.
pub fn drc(layout: &Layout, stack: &Stack) -> Vec<Finding> {
    use rayon::prelude::*;
    layout
        .layers
        .par_iter()
        .flat_map(|l| {
            let Some(sl) = stack.layer_for(l.key.0, l.key.1) else {
                return vec![];
            };
            drc_layer(
                &l.polygons,
                &format!("layer:{}/{}", l.key.0, l.key.1),
                &sl.name,
                sl.min_width,
                sl.min_space,
            )
        })
        .collect()
}

fn mtime(p: &std::path::Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Checks of the project itself: files that are missing or older than their sources.
pub fn project_checks(p: &Project, layout: Option<&Layout>, stack: Option<&Stack>) -> Vec<Finding> {
    let mut out = vec![];
    if let Some(l) = &p.manifest.layout {
        let gds = p.path(&l.gds);
        if !gds.is_file() {
            out.push(Finding::new(
                Severity::Error,
                "project.missing_gds",
                format!(
                    "the layout {} does not exist yet{}",
                    l.gds,
                    if l.generator.is_some() {
                        "; build the layout"
                    } else {
                        ""
                    }
                ),
            ));
        } else if let Some(g) = &l.generator
            && let (Some(a), Some(b)) = (mtime(&p.path(g)), mtime(&gds))
            && a > b
        {
            out.push(
                Finding::new(
                    Severity::Warning,
                    "project.stale_gds",
                    format!(
                        "{g} changed after {} was written; rebuild the layout",
                        l.gds
                    ),
                )
                .on(format!("file:{g}")),
            );
        }
        if !p.path(&l.stack).is_file() {
            out.push(Finding::new(
                Severity::Warning,
                "project.missing_stack",
                format!(
                    "no layer stack at {}; the 3-D view and design rules need one",
                    l.stack
                ),
            ));
        }
        if let (Some(layout), Some(stack)) = (layout, stack) {
            for ld in &layout.layers {
                if stack.layer_for(ld.key.0, ld.key.1).is_none() {
                    out.push(
                        Finding::new(Severity::Info, "stack.unmapped_layer", format!("layer {}/{} ({} polygons) is not in the stack: it has no height or material", ld.key.0, ld.key.1, ld.polygons.len()))
                            .on(format!("layer:{}/{}", ld.key.0, ld.key.1)),
                    );
                }
            }
            for sl in &stack.layers {
                if stack.index_of(&sl.material).is_none()
                    && stack
                        .material(&sl.material)
                        .and_then(|m| m.lightsim.as_ref())
                        .is_none()
                {
                    out.push(Finding::new(
                        Severity::Warning,
                        "stack.unknown_material",
                        format!(
                            "layer {} uses material '{}' with no index and no lightsim model",
                            sl.name, sl.material
                        ),
                    ));
                }
            }
        }
    }
    for s in &p.manifest.simulations {
        let spec = p.path(&s.spec);
        if !spec.is_file() {
            out.push(
                Finding::new(
                    Severity::Error,
                    "project.missing_spec",
                    format!(
                        "simulation {}: {} does not exist yet{}",
                        s.name,
                        s.spec,
                        if s.generator.is_some() {
                            "; build it"
                        } else {
                            ""
                        }
                    ),
                )
                .on(format!("sim:{}", s.name)),
            );
        } else if let Some(g) = &s.generator
            && let (Some(a), Some(b)) = (mtime(&p.path(g)), mtime(&spec))
            && a > b
        {
            out.push(
                Finding::new(
                    Severity::Warning,
                    "project.stale_spec",
                    format!(
                        "simulation {}: {g} changed after {} was written; rebuild it",
                        s.name, s.spec
                    ),
                )
                .on(format!("sim:{}", s.name)),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
        vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
    }

    #[test]
    fn width_and_space() {
        // a 0.5 um wire is fine, a 0.1 um wire violates 0.15 um
        let ok = drc_layer(
            &[rect(0.0, 0.0, 10.0, 0.5)],
            "layer:1/0",
            "core",
            Some(0.15),
            None,
        );
        assert!(ok.is_empty(), "{ok:?}");
        let bad = drc_layer(
            &[rect(0.0, 0.0, 10.0, 0.1)],
            "layer:1/0",
            "core",
            Some(0.15),
            None,
        );
        assert_eq!(bad.len(), 1, "{bad:?}");
        // two wires 0.1 um apart violate 0.15 um spacing; 0.3 um apart do not
        let bad = drc_layer(
            &[rect(0.0, 0.0, 10.0, 0.5), rect(0.0, 0.6, 10.0, 1.1)],
            "l",
            "core",
            None,
            Some(0.15),
        );
        assert_eq!(bad.len(), 1, "{bad:?}");
        let ok = drc_layer(
            &[rect(0.0, 0.0, 10.0, 0.5), rect(0.0, 0.8, 10.0, 1.3)],
            "l",
            "core",
            None,
            Some(0.15),
        );
        assert!(ok.is_empty(), "{ok:?}");
        // an L shape: corners alone are not violations
        let l = vec![
            [0.0, 0.0],
            [5.0, 0.0],
            [5.0, 0.5],
            [0.5, 0.5],
            [0.5, 5.0],
            [0.0, 5.0],
        ];
        assert!(drc_layer(&[l], "l", "core", Some(0.15), Some(0.15)).is_empty());
    }

    #[test]
    fn ring_is_clean() {
        let n = 400;
        let (ro, ri) = (5.225, 4.775);
        let mut p = vec![];
        for k in 0..=n {
            let a = std::f64::consts::PI * k as f64 / n as f64 - std::f64::consts::FRAC_PI_2;
            p.push([ro * a.cos(), ro * a.sin()]);
        }
        for k in (0..=n).rev() {
            let a = std::f64::consts::PI * k as f64 / n as f64 - std::f64::consts::FRAC_PI_2;
            p.push([ri * a.cos(), ri * a.sin()]);
        }
        let f = drc_layer(&[p], "l", "core", Some(0.15), Some(0.15));
        assert!(f.is_empty(), "{f:?}");
    }
}
