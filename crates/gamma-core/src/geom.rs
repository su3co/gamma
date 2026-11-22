//! Polygons, triangulation and extrusion.

pub type P2 = [f64; 2];

#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BBox {
    pub min: P2,
    pub max: P2,
}

impl BBox {
    pub const EMPTY: BBox = BBox { min: [f64::INFINITY, f64::INFINITY], max: [f64::NEG_INFINITY, f64::NEG_INFINITY] };

    pub fn of(pts: &[P2]) -> BBox {
        let mut b = BBox::EMPTY;
        for p in pts {
            b.add(*p);
        }
        b
    }
    pub fn add(&mut self, p: P2) {
        self.min[0] = self.min[0].min(p[0]);
        self.min[1] = self.min[1].min(p[1]);
        self.max[0] = self.max[0].max(p[0]);
        self.max[1] = self.max[1].max(p[1]);
    }
    pub fn union(&mut self, o: &BBox) {
        if o.is_empty() {
            return;
        }
        self.add(o.min);
        self.add(o.max);
    }
    pub fn is_empty(&self) -> bool {
        self.min[0] > self.max[0]
    }
    pub fn contains(&self, p: P2) -> bool {
        p[0] >= self.min[0] && p[0] <= self.max[0] && p[1] >= self.min[1] && p[1] <= self.max[1]
    }
    pub fn intersects(&self, o: &BBox) -> bool {
        !(o.min[0] > self.max[0] || o.max[0] < self.min[0] || o.min[1] > self.max[1] || o.max[1] < self.min[1])
    }
    pub fn width(&self) -> f64 {
        self.max[0] - self.min[0]
    }
    pub fn height(&self) -> f64 {
        self.max[1] - self.min[1]
    }
    pub fn grow(&self, d: f64) -> BBox {
        BBox { min: [self.min[0] - d, self.min[1] - d], max: [self.max[0] + d, self.max[1] + d] }
    }
}

pub fn signed_area(p: &[P2]) -> f64 {
    let n = p.len();
    let mut a = 0.0;
    for i in 0..n {
        let (x0, y0) = (p[i][0], p[i][1]);
        let (x1, y1) = (p[(i + 1) % n][0], p[(i + 1) % n][1]);
        a += x0 * y1 - x1 * y0;
    }
    0.5 * a
}

pub fn point_in_polygon(p: &[P2], q: P2) -> bool {
    let mut inside = false;
    let n = p.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (p[i][0], p[i][1]);
        let (xj, yj) = (p[j][0], p[j][1]);
        if (yi > q[1]) != (yj > q[1]) && q[0] < (xj - xi) * (q[1] - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn dist_point_segment(q: P2, a: P2, b: P2) -> (f64, P2) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 { (((q[0] - a[0]) * dx + (q[1] - a[1]) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    let c = [a[0] + t * dx, a[1] + t * dy];
    (((q[0] - c[0]).powi(2) + (q[1] - c[1]).powi(2)).sqrt(), c)
}

/// Distance from `q` to the outline of polygon `p` and the nearest point on it.
pub fn dist_to_outline(p: &[P2], q: P2) -> (f64, P2) {
    let n = p.len();
    let mut best = (f64::INFINITY, q);
    for i in 0..n {
        let d = dist_point_segment(q, p[i], p[(i + 1) % n]);
        if d.0 < best.0 {
            best = d;
        }
    }
    best
}

/// Triangulate a simple polygon (no holes). Returns vertex indices, three per triangle.
pub fn triangulate(p: &[P2]) -> Vec<u32> {
    if p.len() < 3 {
        return vec![];
    }
    if p.len() == 3 {
        return vec![0, 1, 2];
    }
    if p.len() == 4 && is_rect(p) {
        return vec![0, 1, 2, 0, 2, 3];
    }
    let flat: Vec<f64> = p.iter().flat_map(|q| [q[0], q[1]]).collect();
    match earcutr::earcut(&flat, &[], 2) {
        Ok(ix) => ix.into_iter().map(|i| i as u32).collect(),
        Err(_) => vec![],
    }
}

fn is_rect(p: &[P2]) -> bool {
    let ax = |a: P2, b: P2| a[0] == b[0] || a[1] == b[1];
    ax(p[0], p[1]) && ax(p[1], p[2]) && ax(p[2], p[3]) && ax(p[3], p[0])
}

/// Outline of a GDS path of `width` along `pts` (database units or µm),
/// with flush (`extend = false`) or half-width-extended ends. Miter joins,
/// limited to 4 half-widths.
pub fn path_to_polygon(pts: &[P2], width: f64, extend: bool) -> Vec<Vec<P2>> {
    let mut pts: Vec<P2> = pts.to_vec();
    pts.dedup();
    if pts.len() < 2 || width <= 0.0 {
        return vec![];
    }
    let h = width / 2.0;
    let n = pts.len();
    let dir = |a: P2, b: P2| {
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt();
        [dx / l, dy / l]
    };
    if extend {
        let d0 = dir(pts[1], pts[0]);
        pts[0] = [pts[0][0] + d0[0] * h, pts[0][1] + d0[1] * h];
        let d1 = dir(pts[n - 2], pts[n - 1]);
        pts[n - 1] = [pts[n - 1][0] + d1[0] * h, pts[n - 1][1] + d1[1] * h];
    }
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for i in 0..n {
        let d_in = if i > 0 { dir(pts[i - 1], pts[i]) } else { dir(pts[0], pts[1]) };
        let d_out = if i + 1 < n { dir(pts[i], pts[i + 1]) } else { d_in };
        let n_in = [-d_in[1], d_in[0]];
        let n_out = [-d_out[1], d_out[0]];
        let mut m = [n_in[0] + n_out[0], n_in[1] + n_out[1]];
        let ml = (m[0] * m[0] + m[1] * m[1]).sqrt();
        if ml < 1e-12 {
            m = n_in;
        } else {
            m = [m[0] / ml, m[1] / ml];
        }
        let cos = m[0] * n_in[0] + m[1] * n_in[1];
        let s = (h / cos.max(0.25)).min(4.0 * h);
        left.push([pts[i][0] + m[0] * s, pts[i][1] + m[1] * s]);
        right.push([pts[i][0] - m[0] * s, pts[i][1] - m[1] * s]);
    }
    right.reverse();
    left.extend(right);
    vec![left]
}

/// A triangle mesh with per-vertex normals (for the 3-D view).
#[derive(Clone, Debug, Default)]
pub struct Mesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
}

impl Mesh {
    fn vert(&mut self, p: [f64; 3], n: [f32; 3]) -> u32 {
        let i = (self.positions.len() / 3) as u32;
        self.positions.extend([p[0] as f32, p[1] as f32, p[2] as f32]);
        self.normals.extend(n);
        i
    }

    /// Extrude a polygon from z0 to z1 (top, bottom and side walls).
    pub fn extrude(&mut self, poly: &[P2], z0: f64, z1: f64) {
        if poly.len() < 3 {
            return;
        }
        let ccw = signed_area(poly) >= 0.0;
        let tri = triangulate(poly);
        for (z, nz) in [(z1, 1.0f32), (z0, -1.0f32)] {
            let base: Vec<u32> = poly.iter().map(|p| self.vert([p[0], p[1], z], [0.0, 0.0, nz])).collect();
            for t in tri.chunks_exact(3) {
                let (a, b, c) = (base[t[0] as usize], base[t[1] as usize], base[t[2] as usize]);
                let up = (nz > 0.0) == ccw;
                if up { self.indices.extend([a, b, c]) } else { self.indices.extend([a, c, b]) }
            }
        }
        let n = poly.len();
        for i in 0..n {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l = (dx * dx + dy * dy).sqrt();
            if l == 0.0 {
                continue;
            }
            let s = if ccw { 1.0 } else { -1.0 };
            let nrm = [(s * dy / l) as f32, (-s * dx / l) as f32, 0.0];
            let v0 = self.vert([a[0], a[1], z0], nrm);
            let v1 = self.vert([b[0], b[1], z0], nrm);
            let v2 = self.vert([b[0], b[1], z1], nrm);
            let v3 = self.vert([a[0], a[1], z1], nrm);
            if ccw {
                self.indices.extend([v0, v1, v2, v0, v2, v3]);
            } else {
                self.indices.extend([v0, v2, v1, v0, v3, v2]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tri_and_area() {
        let sq = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [0.0, 1.0]];
        assert_eq!(signed_area(&sq), 2.0);
        assert_eq!(triangulate(&sq).len(), 6);
        let l = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]];
        let t = triangulate(&l);
        assert_eq!(t.len(), 12);
        let area: f64 = t.chunks(3).map(|c| signed_area(&[l[c[0] as usize], l[c[1] as usize], l[c[2] as usize]]).abs()).sum();
        assert!((area - 3.0).abs() < 1e-12);
        assert!(point_in_polygon(&l, [0.5, 1.5]));
        assert!(!point_in_polygon(&l, [1.5, 1.5]));
    }

    #[test]
    fn path_outline() {
        let p = path_to_polygon(&[[0.0, 0.0], [10.0, 0.0]], 2.0, false);
        assert!((signed_area(&p[0]).abs() - 20.0).abs() < 1e-9);
        let p = path_to_polygon(&[[0.0, 0.0], [10.0, 0.0]], 2.0, true);
        assert!((signed_area(&p[0]).abs() - 24.0).abs() < 1e-9);
        let p = path_to_polygon(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], 2.0, false);
        assert!((signed_area(&p[0]).abs() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn extrude_counts() {
        let mut m = Mesh::default();
        m.extrude(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], 0.0, 0.22);
        assert_eq!(m.indices.len(), 6 * 2 + 6 * 4);
    }
}
