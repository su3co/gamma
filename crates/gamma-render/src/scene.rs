//! Layout, stack and map scenes.

use crate::Png;
use crate::colormap::{self, Rgb};
use crate::text::{self, Face};
use anyhow::Result;
use gamma_core::geom::BBox;
use gamma_core::layout::{Layout, layer_color};
use gamma_core::stack::Stack;
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
}

impl Theme {
    pub fn parse(s: &str) -> Theme {
        Theme { dark: s != "light" }
    }
    pub fn ground(&self) -> Rgb {
        if self.dark {
            [12, 14, 18]
        } else {
            [255, 255, 255]
        }
    }
    pub fn ink(&self) -> Rgb {
        if self.dark {
            [236, 238, 242]
        } else {
            [22, 25, 31]
        }
    }
    pub fn ink3(&self) -> Rgb {
        if self.dark {
            [127, 135, 149]
        } else {
            [106, 112, 124]
        }
    }
    pub fn rule(&self) -> Rgb {
        if self.dark {
            [44, 50, 61]
        } else {
            [212, 215, 221]
        }
    }
    pub fn accent(&self) -> Rgb {
        if self.dark {
            [255, 145, 102]
        } else {
            [192, 70, 27]
        }
    }
}

pub(crate) fn rgba(c: Rgb, a: u8) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], a)
}

pub(crate) fn paint(c: Rgb, a: u8) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(rgba(c, a));
    p.anti_alias = true;
    p
}

/// World (µm) to pixel mapping, y up.
#[derive(Clone, Copy)]
pub struct View {
    pub s: f32,
    pub ox: f32,
    pub oy: f32,
    pub h: f32,
}

impl View {
    pub fn fit(b: &BBox, w: u32, h: u32, margin: f32) -> View {
        let bw = b.width().max(1e-9) as f32;
        let bh = b.height().max(1e-9) as f32;
        let s = ((w as f32 - 2.0 * margin) / bw).min((h as f32 - 2.0 * margin) / bh);
        let ox = (w as f32 - s * bw) / 2.0 - s * b.min[0] as f32;
        let oy = (h as f32 - s * bh) / 2.0 - s * b.min[1] as f32;
        View {
            s,
            ox,
            oy,
            h: h as f32,
        }
    }
    pub fn px(&self, p: [f64; 2]) -> (f32, f32) {
        (
            self.ox + self.s * p[0] as f32,
            self.h - (self.oy + self.s * p[1] as f32),
        )
    }
}

pub struct LayoutOptions {
    pub width: u32,
    pub height: u32,
    pub theme: Theme,
    /// Region to show (µm); default the whole layout.
    pub window: Option<BBox>,
    pub highlight: Vec<BBox>,
    pub hidden_layers: Vec<(u16, u16)>,
    pub title: Option<String>,
}

fn scale_bar(pm: &mut Pixmap, v: &View, theme: Theme) {
    let target = 120.0 / v.s; // µm for ~120 px
    let pow = 10f32.powf(target.log10().floor());
    let len = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * pow)
        .find(|l| *l >= target * 0.6)
        .unwrap_or(pow);
    let px = len * v.s;
    let (x0, y0) = (pm.width() as f32 - 24.0 - px, pm.height() as f32 - 24.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x0, y0);
    pb.line_to(x0 + px, y0);
    pb.move_to(x0, y0 - 4.0);
    pb.line_to(x0, y0 + 4.0);
    pb.move_to(x0 + px, y0 - 4.0);
    pb.line_to(x0 + px, y0 + 4.0);
    if let Some(p) = pb.finish() {
        pm.stroke_path(
            &p,
            &paint(theme.ink(), 255),
            &Stroke {
                width: 1.5,
                ..Default::default()
            },
            Transform::identity(),
            None,
        );
    }
    let label = if len >= 1.0 {
        format!("{len} µm")
    } else {
        format!("{} nm", (len * 1000.0).round())
    };
    text::draw(
        pm,
        Face::Mono,
        13.0,
        x0 + px / 2.0,
        y0 - 8.0,
        0.5,
        [theme.ink()[0], theme.ink()[1], theme.ink()[2], 255],
        &label,
    );
}

fn new_pixmap(w: u32, h: u32, theme: Theme) -> Result<Pixmap> {
    let mut pm = Pixmap::new(w.max(16), h.max(16))
        .ok_or_else(|| anyhow::anyhow!("bad image size {w}x{h}"))?;
    pm.fill(rgba(theme.ground(), 255));
    Ok(pm)
}

fn title(pm: &mut Pixmap, theme: Theme, t: &str) {
    let i = theme.ink();
    text::draw(
        pm,
        Face::Sans,
        18.0,
        20.0,
        30.0,
        0.0,
        [i[0], i[1], i[2], 255],
        t,
    );
}

pub fn render_layout(layout: &Layout, stack: Option<&Stack>, o: &LayoutOptions) -> Result<Png> {
    let mut pm = new_pixmap(o.width, o.height, o.theme)?;
    let b = o.window.unwrap_or(layout.bbox).grow(0.0);
    let v = View::fit(&b, o.width, o.height, 48.0);
    // draw layers bottom to top by stack z
    let mut order: Vec<_> = layout
        .layers
        .iter()
        .filter(|l| !o.hidden_layers.contains(&l.key))
        .collect();
    order.sort_by(|a, c| {
        let za = stack
            .and_then(|s| s.layer_for(a.key.0, a.key.1))
            .map(|l| l.zmax())
            .unwrap_or(0.0);
        let zc = stack
            .and_then(|s| s.layer_for(c.key.0, c.key.1))
            .map(|l| l.zmax())
            .unwrap_or(0.0);
        za.partial_cmp(&zc).unwrap()
    });
    for l in order {
        let col = stack
            .and_then(|s| s.layer_for(l.key.0, l.key.1))
            .and_then(|s| s.color.as_deref())
            .and_then(colormap::hex)
            .unwrap_or_else(|| colormap::hex(&layer_color(l.key)).unwrap());
        let mut pb = PathBuilder::new();
        for (i, p) in l.polygons.iter().enumerate() {
            if !l.bboxes[i].intersects(&b) {
                continue;
            }
            let (x, y) = v.px(p[0]);
            pb.move_to(x, y);
            for q in &p[1..] {
                let (x, y) = v.px(*q);
                pb.line_to(x, y);
            }
            pb.close();
        }
        if let Some(path) = pb.finish() {
            pm.fill_path(
                &path,
                &paint(col, 150),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
            pm.stroke_path(
                &path,
                &paint(col, 255),
                &Stroke {
                    width: 1.0,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
        }
    }
    // ports: a short arrow out of the device and the name
    let acc = o.theme.accent();
    for p in &layout.ports {
        let (x, y) = v.px([p.x, p.y]);
        let a = (p.orientation as f32).to_radians();
        let len = 18.0;
        let (dx, dy) = (a.cos() * len, -a.sin() * len);
        let mut pb = PathBuilder::new();
        pb.move_to(x, y);
        pb.line_to(x + dx, y + dy);
        pb.move_to(x + dx - 0.35 * dx + 0.3 * dy, y + dy - 0.35 * dy - 0.3 * dx);
        pb.line_to(x + dx, y + dy);
        pb.line_to(x + dx - 0.35 * dx - 0.3 * dy, y + dy - 0.35 * dy + 0.3 * dx);
        if let Some(path) = pb.finish() {
            pm.stroke_path(
                &path,
                &paint(acc, 255),
                &Stroke {
                    width: 2.0,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
        }
        text::draw(
            &mut pm,
            Face::Mono,
            13.0,
            x + dx * 1.5,
            y + dy * 1.5 + 4.0,
            0.5,
            [acc[0], acc[1], acc[2], 255],
            &p.name,
        );
    }
    for h in &o.highlight {
        let (x0, y0) = v.px([h.min[0], h.max[1]]);
        let (x1, y1) = v.px([h.max[0], h.min[1]]);
        if let Some(r) = Rect::from_ltrb(
            x0 - 4.0,
            y0 - 4.0,
            x1.max(x0 + 1.0) + 4.0,
            y1.max(y0 + 1.0) + 4.0,
        ) {
            let path = PathBuilder::from_rect(r);
            pm.stroke_path(
                &path,
                &paint(o.theme.ink(), 255),
                &Stroke {
                    width: 2.0,
                    dash: tiny_skia::StrokeDash::new(vec![6.0, 4.0], 0.0),
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
        }
    }
    scale_bar(&mut pm, &v, o.theme);
    if let Some(t) = &o.title {
        title(&mut pm, o.theme, t);
    }
    Ok(Png(pm.encode_png()?))
}

/// The extruded stack seen from above at an angle (painter's algorithm).
pub fn render_stack3d(
    layout: &Layout,
    stack: &Stack,
    width: u32,
    height: u32,
    theme: Theme,
    title_text: Option<&str>,
) -> Result<Png> {
    let mut pm = new_pixmap(width, height, theme)?;
    let meshes = layout.extrude(stack);
    let b = layout.bbox;
    let (cx, cy) = (
        0.5 * (b.min[0] + b.max[0]) as f32,
        0.5 * (b.min[1] + b.max[1]) as f32,
    );
    let size = b.width().max(b.height()) as f32;
    // z exaggerated so thin films read
    let zs = (size / 20.0).max(1.0);
    let (az, el) = (-35f32.to_radians(), 55f32.to_radians());
    let project = |p: [f32; 3]| -> (f32, f32, f32) {
        let (x, y, z) = (p[0] - cx, p[1] - cy, p[2] * zs);
        let xr = x * az.cos() - y * az.sin();
        let yr = x * az.sin() + y * az.cos();
        let sy = yr * el.cos() + z * el.sin();
        let depth = yr * el.sin() - z * el.cos();
        (xr, sy, depth)
    };
    let mut tris: Vec<([(f32, f32); 3], f32, Rgb)> = vec![];
    let (mut minx, mut maxx, mut miny, mut maxy) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    let light = [-0.3f32, -0.5, 0.8];
    for (_, color, m) in &meshes {
        let col = colormap::hex(color).unwrap_or([200, 160, 80]);
        for t in m.indices.as_chunks::<3>().0 {
            let mut pts = [(0.0, 0.0); 3];
            let mut d = 0.0;
            let i0 = t[0] as usize;
            for (k, &i) in t.iter().enumerate() {
                let i = i as usize;
                let (x, y, dd) = project([
                    m.positions[3 * i],
                    m.positions[3 * i + 1],
                    m.positions[3 * i + 2],
                ]);
                pts[k] = (x, y);
                d += dd / 3.0;
                minx = minx.min(x);
                maxx = maxx.max(x);
                miny = miny.min(y);
                maxy = maxy.max(y);
            }
            let n = [
                m.normals[3 * i0],
                m.normals[3 * i0 + 1],
                m.normals[3 * i0 + 2],
            ];
            let lam = (n[0] * light[0] + n[1] * light[1] + n[2] * light[2]).abs() * 0.6 + 0.4;
            let c = col.map(|v| (v as f32 * lam).min(255.0) as u8);
            tris.push((pts, d, c));
        }
    }
    tris.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let s = ((width as f32 - 80.0) / (maxx - minx).max(1e-6))
        .min((height as f32 - 80.0) / (maxy - miny).max(1e-6));
    let ox = width as f32 / 2.0 - s * 0.5 * (minx + maxx);
    let oy = height as f32 / 2.0 + s * 0.5 * (miny + maxy);
    for (pts, _, c) in &tris {
        let mut pb = PathBuilder::new();
        pb.move_to(ox + s * pts[0].0, oy - s * pts[0].1);
        pb.line_to(ox + s * pts[1].0, oy - s * pts[1].1);
        pb.line_to(ox + s * pts[2].0, oy - s * pts[2].1);
        pb.close();
        if let Some(p) = pb.finish() {
            let pnt = paint(*c, 255);
            pm.fill_path(&p, &pnt, FillRule::Winding, Transform::identity(), None);
            pm.stroke_path(
                &p,
                &pnt,
                &Stroke {
                    width: 0.6,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
        }
    }
    let i = theme.ink3();
    text::draw(
        &mut pm,
        Face::Mono,
        12.0,
        20.0,
        height as f32 - 20.0,
        0.0,
        [i[0], i[1], i[2], 255],
        &format!("z exaggerated {zs:.0}x"),
    );
    if let Some(t) = title_text {
        title(&mut pm, theme, t);
    }
    Ok(Png(pm.encode_png()?))
}

/// A rectangle drawn over a map (a source, monitor or structure outline), in map coordinates (m).
pub struct Overlay {
    pub label: String,
    pub min: [f64; 2],
    pub max: [f64; 2],
    pub color: Rgb,
}

pub enum MapKind {
    Material,
    Intensity,
    Signed,
}

pub struct MapOptions<'a> {
    pub width: u32,
    pub height: u32,
    pub theme: Theme,
    pub kind: MapKind,
    pub title: Option<&'a str>,
    pub axis_names: [&'a str; 2],
    pub overlays: Vec<Overlay>,
    pub colorbar_label: &'a str,
}

/// A 2-D map `data[iu * nv + iv]` sampled at `u`, `v` (m).
pub fn render_field(u: &[f64], v: &[f64], data: &[f32], o: &MapOptions) -> Result<Png> {
    let mut pm = new_pixmap(o.width, o.height, o.theme)?;
    let (nu, nv) = (u.len(), v.len());
    if nu == 0 || nv == 0 {
        return Ok(Png(pm.encode_png()?));
    }
    let lo = data.iter().cloned().fold(f32::INFINITY, f32::min);
    let hi = data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let amax = lo.abs().max(hi.abs()).max(1e-30);
    let b = BBox {
        min: [u[0] * 1e6, v[0] * 1e6],
        max: [u[nu - 1] * 1e6, v[nv - 1] * 1e6],
    };
    let plot_w = o.width - 90;
    let vw = View::fit(&b, plot_w, o.height - 40, 50.0);
    let du = if nu > 1 { (u[1] - u[0]) * 1e6 } else { 1.0 };
    let dv = if nv > 1 { (v[1] - v[0]) * 1e6 } else { 1.0 };
    let mut img = Pixmap::new(nu as u32, nv as u32).unwrap();
    {
        let px = img.pixels_mut();
        for iu in 0..nu {
            for iv in 0..nv {
                let x = data[iu * nv + iv];
                let c = match o.kind {
                    MapKind::Material => {
                        colormap::material((x - lo) / (hi - lo).max(1e-12), o.theme.dark)
                    }
                    MapKind::Intensity => colormap::intensity(x / hi.max(1e-30), o.theme.dark),
                    MapKind::Signed => colormap::diverging(x / amax, o.theme.dark),
                };
                // flip v so +v is up
                let row = nv - 1 - iv;
                px[row * nu + iu] =
                    tiny_skia::ColorU8::from_rgba(c[0], c[1], c[2], 255).premultiply();
            }
        }
    }
    let (x0, y0) = vw.px([b.min[0] - du / 2.0, b.max[1] + dv / 2.0]);
    let (x1, y1) = vw.px([b.max[0] + du / 2.0, b.min[1] - dv / 2.0]);
    let sx = (x1 - x0) / nu as f32;
    let sy = (y1 - y0) / nv as f32;
    let mut pp = tiny_skia::PixmapPaint::default();
    pp.quality = if sx > 2.0 {
        tiny_skia::FilterQuality::Nearest
    } else {
        tiny_skia::FilterQuality::Bilinear
    };
    pm.draw_pixmap(
        0,
        0,
        img.as_ref(),
        &pp,
        Transform::from_row(sx, 0.0, 0.0, sy, x0, y0),
        None,
    );
    for ov in &o.overlays {
        let (a0, b0) = vw.px([ov.min[0] * 1e6, ov.max[1] * 1e6]);
        let (a1, b1) = vw.px([ov.max[0] * 1e6, ov.min[1] * 1e6]);
        let r = Rect::from_ltrb(
            a0.min(a1) - 0.5,
            b0.min(b1) - 0.5,
            a0.max(a1) + 0.5,
            b0.max(b1) + 0.5,
        );
        if let Some(r) = r {
            pm.stroke_path(
                &PathBuilder::from_rect(r),
                &paint(ov.color, 255),
                &Stroke {
                    width: 1.5,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
            text::draw(
                &mut pm,
                Face::Mono,
                12.0,
                a0.min(a1) + 3.0,
                b0.min(b1) - 4.0,
                0.0,
                [ov.color[0], ov.color[1], ov.color[2], 255],
                &ov.label,
            );
        }
    }
    // axes ticks: extents
    let ink = o.theme.ink3();
    let ic = [ink[0], ink[1], ink[2], 255];
    text::draw(
        &mut pm,
        Face::Mono,
        12.0,
        x0,
        y1 + 18.0,
        0.0,
        ic,
        &format!("{} {:.2} µm", o.axis_names[0], b.min[0]),
    );
    text::draw(
        &mut pm,
        Face::Mono,
        12.0,
        x1,
        y1 + 18.0,
        1.0,
        ic,
        &format!("{:.2} µm", b.max[0]),
    );
    text::draw(
        &mut pm,
        Face::Mono,
        12.0,
        x0 - 6.0,
        y1,
        1.0,
        ic,
        &format!("{} {:.2}", o.axis_names[1], b.min[1]),
    );
    text::draw(
        &mut pm,
        Face::Mono,
        12.0,
        x0 - 6.0,
        y0 + 10.0,
        1.0,
        ic,
        &format!("{:.2}", b.max[1]),
    );
    // colour bar
    let (bx, by0, by1) = (o.width as f32 - 60.0, 60.0, o.height as f32 - 60.0);
    for k in 0..=100 {
        let t = k as f32 / 100.0;
        let c = match o.kind {
            MapKind::Material => colormap::material(t, o.theme.dark),
            MapKind::Intensity => colormap::intensity(t, o.theme.dark),
            MapKind::Signed => colormap::diverging(2.0 * t - 1.0, o.theme.dark),
        };
        let y = by1 - t * (by1 - by0);
        if let Some(r) =
            Rect::from_xywh(bx, y - (by1 - by0) / 100.0, 14.0, (by1 - by0) / 100.0 + 1.0)
        {
            pm.fill_rect(r, &paint(c, 255), Transform::identity(), None);
        }
    }
    let (top, bot) = match o.kind {
        MapKind::Signed => (amax, -amax),
        MapKind::Intensity => (hi, 0.0),
        MapKind::Material => (hi, lo),
    };
    text::draw(
        &mut pm,
        Face::Mono,
        11.0,
        bx + 7.0,
        by0 - 8.0,
        0.5,
        ic,
        &fmt_num(top),
    );
    text::draw(
        &mut pm,
        Face::Mono,
        11.0,
        bx + 7.0,
        by1 + 16.0,
        0.5,
        ic,
        &fmt_num(bot),
    );
    text::draw(
        &mut pm,
        Face::Mono,
        11.0,
        bx + 7.0,
        by1 + 30.0,
        0.5,
        ic,
        o.colorbar_label,
    );
    if let Some(t) = o.title {
        title(&mut pm, o.theme, t);
    }
    Ok(Png(pm.encode_png()?))
}

pub fn fmt_num(x: f32) -> String {
    let a = x.abs();
    if a == 0.0 {
        "0".into()
    } else if !(1e-2..1e4).contains(&a) {
        format!("{x:.2e}")
    } else {
        format!("{x:.3}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gamma_core::gds::Writer;

    #[test]
    fn layout_png() {
        let mut w = Writer::new("t", 1e-9);
        w.begin_cell("top");
        w.polygon((1, 0), &[[0.0, 0.0], [10.0, 0.0], [10.0, 0.5], [0.0, 0.5]]);
        w.end_cell();
        let d = std::env::temp_dir().join(format!("gamma-render-{}.gds", std::process::id()));
        std::fs::write(&d, w.finish()).unwrap();
        let l = Layout::read(&d, None, None).unwrap();
        let s = Stack::soi220();
        let png = render_layout(
            &l,
            Some(&s),
            &LayoutOptions {
                width: 400,
                height: 300,
                theme: Theme { dark: true },
                window: None,
                highlight: vec![l.bbox],
                hidden_layers: vec![],
                title: Some("t".into()),
            },
        )
        .unwrap();
        assert!(png.0.starts_with(&[0x89, b'P', b'N', b'G']));
        let png = render_stack3d(&l, &s, 400, 300, Theme { dark: false }, None).unwrap();
        assert!(png.0.len() > 100);
        let u: Vec<f64> = (0..20).map(|i| i as f64 * 1e-7).collect();
        let data: Vec<f32> = (0..400).map(|i| (i as f32 * 0.1).sin()).collect();
        let png = render_field(
            &u,
            &u,
            &data,
            &MapOptions {
                width: 500,
                height: 400,
                theme: Theme { dark: true },
                kind: MapKind::Signed,
                title: Some("Ey"),
                axis_names: ["x", "y"],
                overlays: vec![],
                colorbar_label: "Ey",
            },
        )
        .unwrap();
        assert!(png.0.len() > 100);
        let _ = std::fs::remove_file(d);
    }
}
