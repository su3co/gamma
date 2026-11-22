//! GDSII stream reader (and a small writer used for tests and fixtures).
//!
//! Reads boundaries, boxes, paths, texts and cell references (SREF/AREF with
//! reflection, magnification and rotation) and flattens a cell into polygons
//! per (layer, datatype), in micrometres.

use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

pub type LayerKey = (u16, u16);

#[derive(Clone, Debug, Default)]
pub struct Library {
    pub name: String,
    /// Size of a database unit in metres.
    pub db_unit_m: f64,
    pub cells: Vec<Cell>,
    index: HashMap<String, usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Cell {
    pub name: String,
    pub boundaries: Vec<RawPoly>,
    pub paths: Vec<RawPath>,
    pub refs: Vec<CellRef>,
    pub texts: Vec<RawText>,
}

#[derive(Clone, Debug)]
pub struct RawPoly {
    pub layer: LayerKey,
    pub pts: Vec<[i32; 2]>,
}

#[derive(Clone, Debug)]
pub struct RawPath {
    pub layer: LayerKey,
    pub width: i32,
    pub pathtype: i16,
    pub pts: Vec<[i32; 2]>,
}

#[derive(Clone, Debug)]
pub struct RawText {
    pub layer: LayerKey,
    pub text: String,
    pub at: [i32; 2],
}

#[derive(Clone, Debug)]
pub struct CellRef {
    pub cell: String,
    pub origin: [i32; 2],
    pub reflect: bool,
    pub mag: f64,
    pub angle_deg: f64,
    /// AREF: columns, rows and the two lattice vectors (database units).
    pub array: Option<(u16, u16, [f64; 2], [f64; 2])>,
}

/// A flattened text label in µm.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Label {
    pub layer: LayerKey,
    pub text: String,
    pub x: f64,
    pub y: f64,
}

/// Affine transform: p' = M p + t.
#[derive(Clone, Copy, Debug)]
pub struct Xform {
    pub m: [[f64; 2]; 2],
    pub t: [f64; 2],
}

impl Xform {
    pub const IDENTITY: Xform = Xform { m: [[1.0, 0.0], [0.0, 1.0]], t: [0.0, 0.0] };

    pub fn apply(&self, p: [f64; 2]) -> [f64; 2] {
        [self.m[0][0] * p[0] + self.m[0][1] * p[1] + self.t[0], self.m[1][0] * p[0] + self.m[1][1] * p[1] + self.t[1]]
    }

    /// self ∘ other (apply other first).
    pub fn then(&self, other: &Xform) -> Xform {
        let a = &self.m;
        let b = &other.m;
        Xform {
            m: [
                [a[0][0] * b[0][0] + a[0][1] * b[1][0], a[0][0] * b[0][1] + a[0][1] * b[1][1]],
                [a[1][0] * b[0][0] + a[1][1] * b[1][0], a[1][0] * b[0][1] + a[1][1] * b[1][1]],
            ],
            t: self.apply(other.t),
        }
    }

    fn of_ref(r: &CellRef, offset: [f64; 2]) -> Xform {
        let (s, c) = r.angle_deg.to_radians().sin_cos();
        let f = if r.reflect { -1.0 } else { 1.0 };
        // reflect about x (y -> -y), scale, rotate, translate
        Xform {
            m: [[r.mag * c, -r.mag * s * f], [r.mag * s, r.mag * c * f]],
            t: [r.origin[0] as f64 + offset[0], r.origin[1] as f64 + offset[1]],
        }
    }

    pub fn is_mirror(&self) -> bool {
        self.m[0][0] * self.m[1][1] - self.m[0][1] * self.m[1][0] < 0.0
    }
}

fn real8(b: &[u8]) -> f64 {
    let sign = if b[0] & 0x80 != 0 { -1.0 } else { 1.0 };
    let exp = (b[0] & 0x7f) as i32 - 64;
    let mut mant: u64 = 0;
    for &x in &b[1..8] {
        mant = (mant << 8) | x as u64;
    }
    sign * (mant as f64) / 2f64.powi(56) * 16f64.powi(exp)
}

fn to_real8(v: f64) -> [u8; 8] {
    if v == 0.0 {
        return [0; 8];
    }
    let sign = if v < 0.0 { 0x80u8 } else { 0 };
    let mut v = v.abs();
    let mut exp: i32 = 64;
    while v >= 1.0 {
        v /= 16.0;
        exp += 1;
    }
    while v < 1.0 / 16.0 {
        v *= 16.0;
        exp -= 1;
    }
    let mant = (v * 2f64.powi(56)).round() as u64;
    let mut out = [0u8; 8];
    out[0] = sign | (exp as u8 & 0x7f);
    for i in 0..7 {
        out[7 - i] = ((mant >> (8 * i)) & 0xff) as u8;
    }
    out
}

impl Library {
    pub fn read(path: &Path) -> Result<Library> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Library::parse(&data).with_context(|| format!("{} is not a readable GDSII file", path.display()))
    }

    pub fn parse(data: &[u8]) -> Result<Library> {
        let mut lib = Library { db_unit_m: 1e-9, ..Default::default() };
        let mut pos = 0usize;
        let mut cell: Option<Cell> = None;
        // current element
        #[derive(Default)]
        struct El {
            kind: u8,
            layer: u16,
            dtype: u16,
            width: i32,
            pathtype: i16,
            xy: Vec<[i32; 2]>,
            sname: String,
            reflect: bool,
            mag: f64,
            angle: f64,
            colrow: (u16, u16),
            text: String,
        }
        let mut el: Option<El> = None;
        while pos + 4 <= data.len() {
            let len = u16::from_be_bytes([data[pos], data[pos + 1]]) as usize;
            let rt = data[pos + 2];
            if len < 4 {
                if len == 0 {
                    break; // padding after ENDLIB
                }
                bail!("corrupt record at byte {pos}");
            }
            if pos + len > data.len() {
                bail!("truncated record at byte {pos}");
            }
            let body = &data[pos + 4..pos + len];
            pos += len;
            let i16s = || body.chunks_exact(2).map(|c| i16::from_be_bytes([c[0], c[1]])).collect::<Vec<_>>();
            let i32s = || body.chunks_exact(4).map(|c| i32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect::<Vec<_>>();
            let string = || String::from_utf8_lossy(body).trim_end_matches('\0').to_string();
            match rt {
                0x02 => lib.name = string(),
                0x03 => {
                    if body.len() >= 16 {
                        lib.db_unit_m = real8(&body[8..16]);
                    }
                }
                0x05 => cell = Some(Cell::default()),
                0x06 => {
                    if let Some(c) = cell.as_mut() {
                        c.name = string();
                    }
                }
                0x07 => {
                    if let Some(c) = cell.take() {
                        lib.index.insert(c.name.clone(), lib.cells.len());
                        lib.cells.push(c);
                    }
                }
                0x08 | 0x09 | 0x0A | 0x0B | 0x0C | 0x2D => el = Some(El { kind: rt, mag: 1.0, ..Default::default() }),
                0x0D => {
                    if let Some(e) = el.as_mut() {
                        e.layer = i16s().first().copied().unwrap_or(0) as u16;
                    }
                }
                0x0E | 0x16 | 0x2E => {
                    if let Some(e) = el.as_mut() {
                        e.dtype = i16s().first().copied().unwrap_or(0) as u16;
                    }
                }
                0x0F => {
                    if let Some(e) = el.as_mut() {
                        e.width = i32s().first().copied().unwrap_or(0);
                    }
                }
                0x21 => {
                    if let Some(e) = el.as_mut() {
                        e.pathtype = i16s().first().copied().unwrap_or(0);
                    }
                }
                0x10 => {
                    if let Some(e) = el.as_mut() {
                        let v = i32s();
                        e.xy = v.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
                    }
                }
                0x12 => {
                    if let Some(e) = el.as_mut() {
                        e.sname = string();
                    }
                }
                0x13 => {
                    if let Some(e) = el.as_mut() {
                        let v = i16s();
                        if v.len() >= 2 {
                            e.colrow = (v[0] as u16, v[1] as u16);
                        }
                    }
                }
                0x19 => {
                    if let Some(e) = el.as_mut() {
                        e.text = string();
                    }
                }
                0x1A => {
                    if let Some(e) = el.as_mut() {
                        if body.len() >= 2 {
                            e.reflect = body[0] & 0x80 != 0;
                        }
                    }
                }
                0x1B => {
                    if let Some(e) = el.as_mut() {
                        if body.len() >= 8 {
                            e.mag = real8(body);
                        }
                    }
                }
                0x1C => {
                    if let Some(e) = el.as_mut() {
                        if body.len() >= 8 {
                            e.angle = real8(body);
                        }
                    }
                }
                0x11 => {
                    let (Some(e), Some(c)) = (el.take(), cell.as_mut()) else { continue };
                    let layer = (e.layer, e.dtype);
                    match e.kind {
                        0x08 | 0x2D => {
                            let mut pts = e.xy;
                            if pts.len() > 1 && pts.first() == pts.last() {
                                pts.pop();
                            }
                            if pts.len() >= 3 {
                                c.boundaries.push(RawPoly { layer, pts });
                            }
                        }
                        0x09 => {
                            if e.xy.len() >= 2 {
                                c.paths.push(RawPath { layer, width: e.width, pathtype: e.pathtype, pts: e.xy })
                            }
                        }
                        0x0C => {
                            if let Some(&at) = e.xy.first() {
                                c.texts.push(RawText { layer, text: e.text, at })
                            }
                        }
                        0x0A => {
                            if let Some(&o) = e.xy.first() {
                                c.refs.push(CellRef { cell: e.sname, origin: o, reflect: e.reflect, mag: e.mag, angle_deg: e.angle, array: None })
                            }
                        }
                        0x0B => {
                            if e.xy.len() >= 3 && e.colrow.0 > 0 && e.colrow.1 > 0 {
                                let (cols, rows) = e.colrow;
                                let o = e.xy[0];
                                let cv = [(e.xy[1][0] - o[0]) as f64 / cols as f64, (e.xy[1][1] - o[1]) as f64 / cols as f64];
                                let rv = [(e.xy[2][0] - o[0]) as f64 / rows as f64, (e.xy[2][1] - o[1]) as f64 / rows as f64];
                                c.refs.push(CellRef {
                                    cell: e.sname,
                                    origin: o,
                                    reflect: e.reflect,
                                    mag: e.mag,
                                    angle_deg: e.angle,
                                    array: Some((cols, rows, cv, rv)),
                                })
                            }
                        }
                        _ => {}
                    }
                }
                0x04 => break,
                _ => {}
            }
        }
        if lib.cells.is_empty() {
            bail!("the library has no cells");
        }
        Ok(lib)
    }

    pub fn cell(&self, name: &str) -> Option<&Cell> {
        self.index.get(name).map(|&i| &self.cells[i])
    }

    /// Cells that no other cell references, largest first (by element count).
    pub fn top_cells(&self) -> Vec<&str> {
        let mut referenced = std::collections::HashSet::new();
        for c in &self.cells {
            for r in &c.refs {
                referenced.insert(r.cell.as_str());
            }
        }
        let mut tops: Vec<&Cell> = self.cells.iter().filter(|c| !referenced.contains(c.name.as_str()) && !c.name.starts_with("$$$")).collect();
        tops.sort_by_key(|c| std::cmp::Reverse(c.refs.len() * 100 + c.boundaries.len() + c.paths.len()));
        tops.into_iter().map(|c| c.name.as_str()).collect()
    }

    /// µm per database unit.
    pub fn um_per_db(&self) -> f64 {
        self.db_unit_m * 1e6
    }

    /// Flatten `top` into polygons per layer (µm) and labels.
    pub fn flatten(&self, top: &str, max_polygons: usize) -> Result<Flat> {
        let mut out = Flat::default();
        let s = self.um_per_db();
        let base = Xform { m: [[s, 0.0], [0.0, s]], t: [0.0, 0.0] };
        let mut stack = vec![];
        self.flatten_rec(top, &base, &mut out, &mut stack, max_polygons)?;
        Ok(out)
    }

    fn flatten_rec(&self, name: &str, xf: &Xform, out: &mut Flat, stack: &mut Vec<String>, max: usize) -> Result<()> {
        let Some(cell) = self.cell(name) else { bail!("cell '{name}' is referenced but not defined") };
        if stack.iter().any(|n| n == name) {
            bail!("cell '{name}' references itself through {}", stack.join(" -> "));
        }
        if stack.len() > 64 {
            bail!("cell hierarchy deeper than 64 levels");
        }
        *out.instances.entry(name.to_string()).or_insert(0) += 1;
        for p in &cell.boundaries {
            let mut pts: Vec<[f64; 2]> = p.pts.iter().map(|q| xf.apply([q[0] as f64, q[1] as f64])).collect();
            if xf.is_mirror() {
                pts.reverse();
            }
            out.push(p.layer, pts, name, max)?;
        }
        for p in &cell.paths {
            let pts: Vec<[f64; 2]> = p.pts.iter().map(|q| [q[0] as f64, q[1] as f64]).collect();
            for poly in crate::geom::path_to_polygon(&pts, p.width.unsigned_abs() as f64, p.pathtype != 0) {
                let poly: Vec<[f64; 2]> = poly.iter().map(|q| xf.apply(*q)).collect();
                out.push(p.layer, poly, name, max)?;
            }
        }
        for t in &cell.texts {
            let q = xf.apply([t.at[0] as f64, t.at[1] as f64]);
            out.labels.push(Label { layer: t.layer, text: t.text.clone(), x: q[0], y: q[1] });
        }
        stack.push(name.to_string());
        for r in &cell.refs {
            match r.array {
                None => self.flatten_rec(&r.cell, &xf.then(&Xform::of_ref(r, [0.0, 0.0])), out, stack, max)?,
                Some((cols, rows, cv, rv)) => {
                    for i in 0..cols {
                        for j in 0..rows {
                            let off = [i as f64 * cv[0] + j as f64 * rv[0], i as f64 * cv[1] + j as f64 * rv[1]];
                            self.flatten_rec(&r.cell, &xf.then(&Xform::of_ref(r, off)), out, stack, max)?;
                        }
                    }
                }
            }
        }
        stack.pop();
        Ok(())
    }
}

/// A flattened cell.
#[derive(Clone, Debug, Default)]
pub struct Flat {
    /// Polygons per layer, µm, counter-clockwise or as written.
    pub layers: BTreeMap<LayerKey, Vec<Vec<[f64; 2]>>>,
    /// For each polygon (same order as `layers`), the cell it came from.
    pub owners: BTreeMap<LayerKey, Vec<String>>,
    pub labels: Vec<Label>,
    /// Number of placements of each cell.
    pub instances: BTreeMap<String, usize>,
    count: usize,
}

impl Flat {
    fn push(&mut self, layer: LayerKey, pts: Vec<[f64; 2]>, owner: &str, max: usize) -> Result<()> {
        self.count += 1;
        if self.count > max {
            bail!("the layout has more than {max} polygons after flattening; open a sub-cell instead");
        }
        self.layers.entry(layer).or_default().push(pts);
        self.owners.entry(layer).or_default().push(owner.to_string());
        Ok(())
    }

    pub fn polygon_count(&self) -> usize {
        self.count
    }
}

/// Minimal GDSII writer: one or more cells of boundaries and references.
pub struct Writer {
    buf: Vec<u8>,
    db_unit_m: f64,
}

impl Writer {
    pub fn new(libname: &str, db_unit_m: f64) -> Writer {
        let mut w = Writer { buf: vec![], db_unit_m };
        w.rec(0x00, 0x02, &600i16.to_be_bytes());
        w.rec(0x01, 0x02, &[0u8; 24]);
        w.str_rec(0x02, libname);
        let mut units = vec![];
        units.extend(to_real8(db_unit_m / 1e-6));
        units.extend(to_real8(db_unit_m));
        w.rec(0x03, 0x05, &units);
        w
    }

    fn rec(&mut self, rt: u8, dt: u8, body: &[u8]) {
        let len = (body.len() + 4) as u16;
        self.buf.extend(len.to_be_bytes());
        self.buf.push(rt);
        self.buf.push(dt);
        self.buf.extend(body);
    }

    fn str_rec(&mut self, rt: u8, s: &str) {
        let mut b = s.as_bytes().to_vec();
        if b.len() % 2 == 1 {
            b.push(0);
        }
        self.rec(rt, 0x06, &b);
    }

    fn db(&self, um: f64) -> i32 {
        (um * 1e-6 / self.db_unit_m).round() as i32
    }

    pub fn begin_cell(&mut self, name: &str) {
        self.rec(0x05, 0x02, &[0u8; 24]);
        self.str_rec(0x06, name);
    }

    pub fn end_cell(&mut self) {
        self.rec(0x07, 0x00, &[]);
    }

    /// A polygon in µm.
    pub fn polygon(&mut self, layer: LayerKey, pts: &[[f64; 2]]) {
        self.rec(0x08, 0x00, &[]);
        self.rec(0x0D, 0x02, &(layer.0 as i16).to_be_bytes());
        self.rec(0x0E, 0x02, &(layer.1 as i16).to_be_bytes());
        let mut xy = vec![];
        for p in pts.iter().chain(pts.first()) {
            xy.extend(self.db(p[0]).to_be_bytes());
            xy.extend(self.db(p[1]).to_be_bytes());
        }
        self.rec(0x10, 0x03, &xy);
        self.rec(0x11, 0x00, &[]);
    }

    pub fn text(&mut self, layer: LayerKey, text: &str, at: [f64; 2]) {
        self.rec(0x0C, 0x00, &[]);
        self.rec(0x0D, 0x02, &(layer.0 as i16).to_be_bytes());
        self.rec(0x16, 0x02, &(layer.1 as i16).to_be_bytes());
        let mut xy = vec![];
        xy.extend(self.db(at[0]).to_be_bytes());
        xy.extend(self.db(at[1]).to_be_bytes());
        self.rec(0x10, 0x03, &xy);
        self.str_rec(0x19, text);
        self.rec(0x11, 0x00, &[]);
    }

    /// A reference to `cell` at `at` (µm), rotated by `angle_deg`, optionally mirrored.
    pub fn sref(&mut self, cell: &str, at: [f64; 2], angle_deg: f64, reflect: bool) {
        self.rec(0x0A, 0x00, &[]);
        self.str_rec(0x12, cell);
        if angle_deg != 0.0 || reflect {
            self.rec(0x1A, 0x01, &[if reflect { 0x80 } else { 0 }, 0]);
            if angle_deg != 0.0 {
                self.rec(0x1C, 0x05, &to_real8(angle_deg));
            }
        }
        let mut xy = vec![];
        xy.extend(self.db(at[0]).to_be_bytes());
        xy.extend(self.db(at[1]).to_be_bytes());
        self.rec(0x10, 0x03, &xy);
        self.rec(0x11, 0x00, &[]);
    }

    pub fn finish(mut self) -> Vec<u8> {
        self.rec(0x04, 0x00, &[]);
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real8_roundtrip() {
        for v in [1e-9, 1e-3, 0.001, 1.0, 90.0, 270.0, -2.5, 1e-6] {
            assert!((real8(&to_real8(v)) - v).abs() <= v.abs() * 1e-12, "{v}");
        }
    }

    #[test]
    fn hierarchy_flatten() {
        let mut w = Writer::new("lib", 1e-9);
        w.begin_cell("unit");
        w.polygon((1, 0), &[[0.0, 0.0], [1.0, 0.0], [1.0, 0.5], [0.0, 0.5]]);
        w.end_cell();
        w.begin_cell("top");
        w.sref("unit", [10.0, 0.0], 90.0, false);
        w.sref("unit", [0.0, 0.0], 0.0, true);
        w.text((10, 0), "o1", [0.0, 0.25]);
        w.end_cell();
        let lib = Library::parse(&w.finish()).unwrap();
        assert_eq!(lib.top_cells(), vec!["top"]);
        let f = lib.flatten("top", 1000).unwrap();
        let polys = &f.layers[&(1, 0)];
        assert_eq!(polys.len(), 2);
        // rotated by 90 deg about the origin and moved to x = 10: x in [9.5, 10], y in [0, 1]
        let xs: Vec<f64> = polys[0].iter().map(|p| p[0]).collect();
        let ys: Vec<f64> = polys[0].iter().map(|p| p[1]).collect();
        assert!((xs.iter().cloned().fold(f64::MAX, f64::min) - 9.5).abs() < 1e-9);
        assert!((ys.iter().cloned().fold(f64::MIN, f64::max) - 1.0).abs() < 1e-9);
        // mirrored about x: y in [-0.5, 0]
        let ys: Vec<f64> = polys[1].iter().map(|p| p[1]).collect();
        assert!((ys.iter().cloned().fold(f64::MAX, f64::min) + 0.5).abs() < 1e-9);
        assert_eq!(f.labels[0].text, "o1");
        assert_eq!(f.instances["unit"], 2);
    }

    #[test]
    fn self_reference_is_an_error() {
        let mut w = Writer::new("lib", 1e-9);
        w.begin_cell("a");
        w.sref("a", [0.0, 0.0], 0.0, false);
        w.end_cell();
        let lib = Library::parse(&w.finish()).unwrap();
        assert!(lib.flatten("a", 10).is_err());
    }
}
