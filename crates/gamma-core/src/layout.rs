//! The layout model the viewers and tools query.

use crate::gds::{Flat, Label, LayerKey, Library};
use crate::geom::{self, BBox, P2};
use crate::project::Project;
use crate::refs::ObjRef;
use crate::stack::Stack;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const MAX_POLYGONS: usize = 5_000_000;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Port {
    pub name: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    /// Direction the port faces, degrees (0 = +x, 90 = +y).
    #[serde(default)]
    pub orientation: f64,
    #[serde(default)]
    pub layer: Option<[u16; 2]>,
}

#[derive(Clone, Debug)]
pub struct LayerData {
    pub key: LayerKey,
    pub polygons: Vec<Vec<P2>>,
    pub owners: Vec<String>,
    pub bboxes: Vec<BBox>,
    pub bbox: BBox,
}

#[derive(Clone, Debug)]
pub struct Layout {
    pub top: String,
    pub cells: Vec<CellInfo>,
    pub layers: Vec<LayerData>,
    pub ports: Vec<Port>,
    pub labels: Vec<Label>,
    pub bbox: BBox,
    pub db_unit_m: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct CellInfo {
    pub name: String,
    pub instances: usize,
    pub children: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LayerSummary {
    pub layer: [u16; 2],
    pub reference: String,
    pub name: Option<String>,
    pub material: Option<String>,
    pub color: String,
    pub polygons: usize,
    pub bbox: BBox,
    pub zmin: Option<f64>,
    pub zmax: Option<f64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub top: String,
    pub cells: Vec<CellInfo>,
    pub layers: Vec<LayerSummary>,
    pub ports: Vec<Port>,
    pub labels: usize,
    pub polygons: usize,
    pub bbox: BBox,
    pub db_unit_nm: f64,
}

/// Default colour for layers the stack does not describe.
pub fn layer_color(key: LayerKey) -> String {
    const PALETTE: [&str; 10] = [
        "#d9a441", "#5fb39a", "#c26a8a", "#6c8fd6", "#c27b2a", "#8f7fd1", "#7aa35a", "#d1665a",
        "#4fa3b8", "#b08f5a",
    ];
    PALETTE[(key.0 as usize * 7 + key.1 as usize * 3) % PALETTE.len()].to_string()
}

impl Layout {
    pub fn from_flat(
        flat: Flat,
        top: &str,
        cells: Vec<CellInfo>,
        ports: Vec<Port>,
        db_unit_m: f64,
    ) -> Layout {
        let mut bbox = BBox::EMPTY;
        let mut layers = vec![];
        for (key, polys) in flat.layers {
            let owners = flat.owners.get(&key).cloned().unwrap_or_default();
            let bboxes: Vec<BBox> = polys.iter().map(|p| BBox::of(p)).collect();
            let mut lb = BBox::EMPTY;
            for b in &bboxes {
                lb.union(b);
            }
            bbox.union(&lb);
            layers.push(LayerData {
                key,
                polygons: polys,
                owners,
                bboxes,
                bbox: lb,
            });
        }
        for p in &ports {
            bbox.add([p.x, p.y]);
        }
        Layout {
            top: top.to_string(),
            cells,
            layers,
            ports,
            labels: flat.labels,
            bbox,
            db_unit_m,
        }
    }

    /// Read a GDS file, flatten `top` (or the main top cell), and attach ports.
    pub fn read(gds: &Path, top: Option<&str>, ports_file: Option<&Path>) -> Result<Layout> {
        let lib = Library::read(gds)?;
        let top = match top {
            Some(t) => {
                if lib.cell(t).is_none() {
                    bail!(
                        "cell '{t}' is not in {}; cells: {}",
                        gds.display(),
                        lib.cells
                            .iter()
                            .map(|c| c.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                t.to_string()
            }
            None => lib
                .top_cells()
                .first()
                .map(|s| s.to_string())
                .context("the GDS file has no top cell")?,
        };
        let flat = lib.flatten(&top, MAX_POLYGONS)?;
        let cells = flat
            .instances
            .iter()
            .map(|(name, &n)| {
                let mut children: Vec<String> = lib
                    .cell(name)
                    .map(|c| c.refs.iter().map(|r| r.cell.clone()).collect())
                    .unwrap_or_default();
                children.sort();
                children.dedup();
                CellInfo {
                    name: name.clone(),
                    instances: n,
                    children,
                }
            })
            .collect();
        let ports = match ports_file {
            Some(p) if p.is_file() => {
                let text = std::fs::read_to_string(p)?;
                parse_ports(&text).with_context(|| format!("reading ports from {}", p.display()))?
            }
            // gdsfactory files carry their ports in the GDS context
            _ => lib
                .kfactory_ports(&top)
                .into_iter()
                .map(|(name, x, y, width, orientation)| Port {
                    name,
                    x,
                    y,
                    width,
                    orientation,
                    layer: None,
                })
                .collect(),
        };
        Ok(Layout::from_flat(flat, &top, cells, ports, lib.db_unit_m))
    }

    /// The layout of a project, or `None` if the project has no layout yet.
    pub fn of_project(p: &Project) -> Result<Option<Layout>> {
        let Some(l) = &p.manifest.layout else {
            return Ok(None);
        };
        let gds = p.path(&l.gds);
        if !gds.is_file() {
            return Ok(None);
        }
        let ports = l.ports.as_ref().map(|s| p.path(s));
        Layout::read(&gds, l.top.as_deref(), ports.as_deref()).map(Some)
    }

    pub fn layer(&self, key: LayerKey) -> Option<&LayerData> {
        self.layers.iter().find(|l| l.key == key)
    }

    pub fn polygon_count(&self) -> usize {
        self.layers.iter().map(|l| l.polygons.len()).sum()
    }

    pub fn summary(&self, stack: Option<&Stack>) -> Summary {
        let layers = self
            .layers
            .iter()
            .map(|l| {
                let sl = stack.and_then(|s| s.layer_for(l.key.0, l.key.1));
                LayerSummary {
                    layer: [l.key.0, l.key.1],
                    reference: ObjRef::Layer {
                        layer: l.key.0,
                        datatype: l.key.1,
                    }
                    .to_string(),
                    name: sl.map(|s| s.name.clone()),
                    material: sl.map(|s| s.material.clone()),
                    color: sl
                        .and_then(|s| s.color.clone())
                        .unwrap_or_else(|| layer_color(l.key)),
                    polygons: l.polygons.len(),
                    bbox: l.bbox,
                    zmin: sl.map(|s| s.zmin),
                    zmax: sl.map(|s| s.zmax()),
                }
            })
            .collect();
        Summary {
            top: self.top.clone(),
            cells: self.cells.clone(),
            layers,
            ports: self.ports.clone(),
            labels: self.labels.len(),
            polygons: self.polygon_count(),
            bbox: self.bbox,
            db_unit_nm: self.db_unit_m * 1e9,
        }
    }

    /// Triangulated layer for the viewer: `[u32 nverts][u32 nidx][f32 x,y ...][u32 idx ...][u32 poly-of-triangle ...]`, little endian.
    pub fn layer_mesh_bytes(&self, key: LayerKey) -> Option<Vec<u8>> {
        let l = self.layer(key)?;
        let tris: Vec<Vec<u32>> = {
            use rayon::prelude::*;
            l.polygons
                .par_iter()
                .map(|p| geom::triangulate(p))
                .collect()
        };
        let nverts: usize = l.polygons.iter().map(|p| p.len()).sum();
        let nidx: usize = tris.iter().map(|t| t.len()).sum();
        let mut out = Vec::with_capacity(8 + nverts * 8 + nidx * 4 + nidx / 3 * 4);
        out.extend((nverts as u32).to_le_bytes());
        out.extend((nidx as u32).to_le_bytes());
        for p in &l.polygons {
            for q in p {
                out.extend((q[0] as f32).to_le_bytes());
                out.extend((q[1] as f32).to_le_bytes());
            }
        }
        let mut base = 0u32;
        for (p, t) in l.polygons.iter().zip(&tris) {
            for &i in t {
                out.extend((base + i).to_le_bytes());
            }
            base += p.len() as u32;
        }
        for (pi, t) in tris.iter().enumerate() {
            for _ in 0..t.len() / 3 {
                out.extend((pi as u32).to_le_bytes());
            }
        }
        Some(out)
    }

    /// Polygon outlines of a layer as line segments for the viewer:
    /// `[u32 nverts][f32 x,y ...]` with consecutive pairs forming segments.
    pub fn layer_outline_bytes(&self, key: LayerKey) -> Option<Vec<u8>> {
        let l = self.layer(key)?;
        let nseg: usize = l.polygons.iter().map(|p| p.len()).sum();
        let mut out = Vec::with_capacity(4 + nseg * 16);
        out.extend(((nseg * 2) as u32).to_le_bytes());
        for p in &l.polygons {
            for i in 0..p.len() {
                let a = p[i];
                let b = p[(i + 1) % p.len()];
                for q in [a, b] {
                    out.extend((q[0] as f32).to_le_bytes());
                    out.extend((q[1] as f32).to_le_bytes());
                }
            }
        }
        Some(out)
    }

    /// What is under the point (µm): a port within `tol`, else the top-most
    /// polygon by stack order (highest layer first).
    pub fn pick(&self, q: P2, tol: f64, stack: Option<&Stack>) -> Option<Picked> {
        for p in &self.ports {
            let r = (p.width / 2.0).max(tol);
            if (p.x - q[0]).hypot(p.y - q[1]) <= r {
                return Some(Picked {
                    reference: ObjRef::Port {
                        name: p.name.clone(),
                    }
                    .to_string(),
                    description: describe_port(p),
                    bbox: BBox {
                        min: [p.x - r, p.y - r],
                        max: [p.x + r, p.y + r],
                    },
                });
            }
        }
        let mut order: Vec<&LayerData> = self.layers.iter().collect();
        order.sort_by(|a, b| {
            let za = stack
                .and_then(|s| s.layer_for(a.key.0, a.key.1))
                .map(|l| l.zmax())
                .unwrap_or(0.0);
            let zb = stack
                .and_then(|s| s.layer_for(b.key.0, b.key.1))
                .map(|l| l.zmax())
                .unwrap_or(0.0);
            zb.partial_cmp(&za).unwrap_or(std::cmp::Ordering::Equal)
        });
        for l in order {
            if !l.bbox.grow(tol).contains(q) {
                continue;
            }
            for (i, p) in l.polygons.iter().enumerate().rev() {
                if !l.bboxes[i].grow(tol).contains(q) {
                    continue;
                }
                if geom::point_in_polygon(p, q) || geom::dist_to_outline(p, q).0 <= tol {
                    let b = l.bboxes[i];
                    let name = stack
                        .and_then(|s| s.layer_for(l.key.0, l.key.1))
                        .map(|s| s.name.clone())
                        .unwrap_or_else(|| format!("{}/{}", l.key.0, l.key.1));
                    return Some(Picked {
                        reference: ObjRef::Poly {
                            layer: l.key.0,
                            datatype: l.key.1,
                            index: i,
                        }
                        .to_string(),
                        description: format!(
                            "polygon {i} on layer {name} from cell {}: {} vertices, {:.3} x {:.3} um, area {:.4} um^2",
                            l.owners.get(i).map(|s| s.as_str()).unwrap_or("?"),
                            p.len(),
                            b.width(),
                            b.height(),
                            geom::signed_area(p).abs()
                        ),
                        bbox: b,
                    });
                }
            }
        }
        None
    }

    /// Smallest edge-to-edge distance between two polygons (0 if they overlap).
    pub fn gap(&self, a: (LayerKey, usize), b: (LayerKey, usize)) -> Option<(f64, P2, P2)> {
        let pa = self.layer(a.0)?.polygons.get(a.1)?;
        let pb = self.layer(b.0)?.polygons.get(b.1)?;
        if pa.iter().any(|q| geom::point_in_polygon(pb, *q))
            || pb.iter().any(|q| geom::point_in_polygon(pa, *q))
        {
            return Some((0.0, pa[0], pa[0]));
        }
        let mut best = (f64::INFINITY, [0.0; 2], [0.0; 2]);
        for (x, y) in [(pa, pb), (pb, pa)] {
            for q in x {
                let (d, c) = geom::dist_to_outline(y, *q);
                if d < best.0 {
                    best = (d, *q, c);
                }
            }
        }
        Some(best)
    }

    /// Polygons of a layer intersecting a box (indices).
    pub fn query(&self, key: LayerKey, b: &BBox) -> Vec<usize> {
        self.layer(key)
            .map(|l| {
                (0..l.polygons.len())
                    .filter(|&i| l.bboxes[i].intersects(b))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Bounding box of an object reference, for highlighting.
    pub fn bbox_of(&self, r: &ObjRef) -> Option<BBox> {
        match r {
            ObjRef::Port { name } => self.ports.iter().find(|p| &p.name == name).map(|p| {
                let w = p.width.max(0.2) / 2.0;
                BBox {
                    min: [p.x - w, p.y - w],
                    max: [p.x + w, p.y + w],
                }
            }),
            ObjRef::Poly {
                layer,
                datatype,
                index,
            } => self
                .layer((*layer, *datatype))
                .and_then(|l| l.bboxes.get(*index).copied()),
            ObjRef::Layer { layer, datatype } => self.layer((*layer, *datatype)).map(|l| l.bbox),
            ObjRef::Cell { .. } => Some(self.bbox),
            ObjRef::Point { x, y } => Some(BBox {
                min: [*x, *y],
                max: [*x, *y],
            }),
            _ => None,
        }
    }

    /// Extruded 3-D mesh of the layers the stack places in z.
    pub fn extrude(&self, stack: &Stack) -> Vec<(LayerKey, String, geom::Mesh)> {
        let mut out = vec![];
        for l in &self.layers {
            let Some(sl) = stack.layer_for(l.key.0, l.key.1) else {
                continue;
            };
            let mut m = geom::Mesh::default();
            for p in &l.polygons {
                m.extrude(p, sl.zmin, sl.zmax());
            }
            out.push((
                l.key,
                sl.color.clone().unwrap_or_else(|| layer_color(l.key)),
                m,
            ));
        }
        out
    }
}

pub fn describe_port(p: &Port) -> String {
    format!(
        "port {} at ({:.3}, {:.3}) um, width {:.3} um, facing {:.0} deg{}",
        p.name,
        p.x,
        p.y,
        p.width,
        p.orientation,
        p.layer
            .map(|l| format!(", layer {}/{}", l[0], l[1]))
            .unwrap_or_default()
    )
}

#[derive(Clone, Debug, Serialize)]
pub struct Picked {
    pub reference: String,
    pub description: String,
    pub bbox: BBox,
}

/// Ports JSON: a list of ports, or `{"ports": [...]}`; gdsfactory-style
/// `center: [x, y]` is accepted too.
pub fn parse_ports(text: &str) -> Result<Vec<Port>> {
    let v: serde_json::Value = serde_json::from_str(text)?;
    let list = v.get("ports").cloned().unwrap_or(v);
    let arr = list.as_array().context("expected a list of ports")?;
    let mut out = vec![];
    for p in arr {
        let mut p = p.clone();
        if let Some(c) = p.get("center").and_then(|c| c.as_array()).cloned() {
            p["x"] = c[0].clone();
            p["y"] = c[1].clone();
        }
        out.push(serde_json::from_value::<Port>(p)?);
    }
    Ok(out)
}

/// Polygon key for diffs: vertices rounded to 1 nm, rotated to start at the
/// smallest vertex and oriented counter-clockwise.
fn canon(p: &[P2]) -> Vec<(i64, i64)> {
    let mut v: Vec<(i64, i64)> = p
        .iter()
        .map(|q| {
            (
                (q[0] * 1000.0).round() as i64,
                (q[1] * 1000.0).round() as i64,
            )
        })
        .collect();
    v.dedup();
    if v.len() > 1 && v.first() == v.last() {
        v.pop();
    }
    if geom::signed_area(p) < 0.0 {
        v.reverse();
    }
    if let Some((i, _)) = v.iter().enumerate().min_by_key(|(_, q)| **q) {
        v.rotate_left(i);
    }
    v
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LayerDiff {
    pub layer: [u16; 2],
    pub added: Vec<usize>,
    pub removed: Vec<usize>,
    pub added_area: f64,
    pub removed_area: f64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LayoutDiff {
    pub layers: Vec<LayerDiff>,
    pub ports_added: Vec<String>,
    pub ports_removed: Vec<String>,
    pub ports_moved: Vec<String>,
    pub bbox_before: BBox,
    pub bbox_after: BBox,
}

impl LayoutDiff {
    pub fn is_empty(&self) -> bool {
        self.layers
            .iter()
            .all(|l| l.added.is_empty() && l.removed.is_empty())
            && self.ports_added.is_empty()
            && self.ports_removed.is_empty()
            && self.ports_moved.is_empty()
    }
}

/// Polygons added (indices into `after`) and removed (indices into `before`) per layer.
pub fn diff(before: &Layout, after: &Layout) -> LayoutDiff {
    let keys: BTreeSet<LayerKey> = before
        .layers
        .iter()
        .chain(&after.layers)
        .map(|l| l.key)
        .collect();
    let mut out = LayoutDiff {
        bbox_before: before.bbox,
        bbox_after: after.bbox,
        ..Default::default()
    };
    for k in keys {
        let empty = vec![];
        let a = before.layer(k).map(|l| &l.polygons).unwrap_or(&empty);
        let b = after.layer(k).map(|l| &l.polygons).unwrap_or(&empty);
        let mut count: BTreeMap<Vec<(i64, i64)>, i64> = BTreeMap::new();
        for p in a {
            *count.entry(canon(p)).or_default() += 1;
        }
        let mut added = vec![];
        for (i, p) in b.iter().enumerate() {
            let c = count.entry(canon(p)).or_default();
            if *c > 0 { *c -= 1 } else { added.push(i) }
        }
        let mut count_b: BTreeMap<Vec<(i64, i64)>, i64> = BTreeMap::new();
        for p in b {
            *count_b.entry(canon(p)).or_default() += 1;
        }
        let mut removed = vec![];
        for (i, p) in a.iter().enumerate() {
            let c = count_b.entry(canon(p)).or_default();
            if *c > 0 { *c -= 1 } else { removed.push(i) }
        }
        if !added.is_empty() || !removed.is_empty() {
            out.layers.push(LayerDiff {
                layer: [k.0, k.1],
                added_area: added.iter().map(|&i| geom::signed_area(&b[i]).abs()).sum(),
                removed_area: removed
                    .iter()
                    .map(|&i| geom::signed_area(&a[i]).abs())
                    .sum(),
                added,
                removed,
            });
        }
    }
    for p in &after.ports {
        match before.ports.iter().find(|q| q.name == p.name) {
            None => out.ports_added.push(p.name.clone()),
            Some(q)
                if (q.x - p.x).abs() > 1e-4
                    || (q.y - p.y).abs() > 1e-4
                    || (q.orientation - p.orientation).abs() > 1e-6 =>
            {
                out.ports_moved.push(p.name.clone())
            }
            _ => {}
        }
    }
    for q in &before.ports {
        if !after.ports.iter().any(|p| p.name == q.name) {
            out.ports_removed.push(q.name.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gds::Writer;

    fn lib(shift: f64) -> Layout {
        let mut w = Writer::new("t", 1e-9);
        w.begin_cell("top");
        w.polygon((1, 0), &[[0.0, 0.0], [10.0, 0.0], [10.0, 0.5], [0.0, 0.5]]);
        w.polygon(
            (1, 0),
            &[
                [0.0, 1.0 + shift],
                [10.0, 1.0 + shift],
                [10.0, 1.5 + shift],
                [0.0, 1.5 + shift],
            ],
        );
        w.end_cell();
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("a.gds");
        std::fs::write(&p, w.finish()).unwrap();
        let ports = d.path().join("p.json");
        std::fs::write(
            &ports,
            r#"[{"name":"o1","center":[0,0.25],"width":0.5,"orientation":180,"layer":[1,0]}]"#,
        )
        .unwrap();
        Layout::read(&p, None, Some(&ports)).unwrap()
    }

    #[test]
    fn pick_gap_diff() {
        let a = lib(0.0);
        assert_eq!(a.ports[0].name, "o1");
        let s = Stack::soi220();
        let p = a.pick([5.0, 0.25], 0.01, Some(&s)).unwrap();
        assert_eq!(p.reference, "poly:1/0#0");
        assert_eq!(
            a.pick([0.0, 0.25], 0.05, Some(&s)).unwrap().reference,
            "port:o1"
        );
        assert!(a.pick([5.0, 0.75], 0.01, Some(&s)).is_none());
        let (g, _, _) = a.gap(((1, 0), 0), ((1, 0), 1)).unwrap();
        assert!((g - 0.5).abs() < 1e-6);
        let b = lib(0.1);
        let d = diff(&a, &b);
        assert_eq!(d.layers[0].added, vec![1]);
        assert_eq!(d.layers[0].removed, vec![1]);
        assert!(diff(&a, &lib(0.0)).is_empty());
        let mesh = a.layer_mesh_bytes((1, 0)).unwrap();
        assert_eq!(u32::from_le_bytes(mesh[0..4].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(mesh[4..8].try_into().unwrap()), 12);
        assert_eq!(a.extrude(&s)[0].2.indices.len(), 2 * 36);
    }
}

#[cfg(test)]
mod kfactory_tests {
    #[test]
    fn ports_from_gdsfactory_gds() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/wire/build/wire.gds");
        let l = super::Layout::read(&p, None, None).unwrap();
        assert_eq!(l.ports.len(), 2);
        assert_eq!(l.ports[0].name, "o1");
        assert!((l.ports[1].x - 5.0).abs() < 1e-9);
        assert!((l.ports[0].orientation - 180.0).abs() < 1e-9);
        assert!((l.ports[0].width - 0.45).abs() < 1e-9);
    }
}
