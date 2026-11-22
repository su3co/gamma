//! Line plots (spectra, S-parameters, time series, decay).

use crate::Png;
use crate::scene::{Theme, fmt_num, paint};
use crate::text::{self, Face};
use anyhow::Result;
use tiny_skia::{PathBuilder, Pixmap, Stroke, Transform};

pub struct Line<'a> {
    pub name: &'a str,
    pub y: &'a [f64],
}

pub struct PlotOptions<'a> {
    pub width: u32,
    pub height: u32,
    pub theme: Theme,
    pub title: Option<&'a str>,
    pub xlabel: &'a str,
    pub ylabel: &'a str,
    pub log_y: bool,
}

const SERIES_DARK: [[u8; 3]; 6] = [
    [255, 145, 102],
    [79, 209, 181],
    [169, 155, 255],
    [227, 181, 86],
    [236, 238, 242],
    [120, 170, 235],
];
const SERIES_LIGHT: [[u8; 3]; 6] = [
    [192, 70, 27],
    [15, 123, 108],
    [59, 42, 140],
    [127, 90, 16],
    [22, 25, 31],
    [40, 90, 180],
];

pub fn series_color(i: usize, theme: Theme) -> [u8; 3] {
    if theme.dark {
        SERIES_DARK[i % 6]
    } else {
        SERIES_LIGHT[i % 6]
    }
}

fn nice_ticks(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    let span = (hi - lo).max(1e-300);
    let raw = span / n as f64;
    let p = 10f64.powf(raw.log10().floor());
    let step = [1.0, 2.0, 5.0, 10.0]
        .iter()
        .map(|m| m * p)
        .find(|s| *s >= raw)
        .unwrap_or(p);
    let start = (lo / step).ceil() * step;
    let mut out = vec![];
    let mut t = start;
    while t <= hi + step * 1e-9 {
        out.push(t);
        t += step;
    }
    out
}

pub fn render_plot(x: &[f64], lines: &[Line], o: &PlotOptions) -> Result<Png> {
    let mut pm = Pixmap::new(o.width, o.height).ok_or_else(|| anyhow::anyhow!("bad size"))?;
    let g = o.theme.ground();
    pm.fill(tiny_skia::Color::from_rgba8(g[0], g[1], g[2], 255));
    let (l, r, t, b) = (
        78.0f32,
        o.width as f32 - 24.0,
        if o.title.is_some() { 56.0 } else { 24.0 },
        o.height as f32 - 56.0,
    );
    let tr = |v: f64| if o.log_y { v.max(1e-300).log10() } else { v };
    let (mut ylo, mut yhi) = (f64::INFINITY, f64::NEG_INFINITY);
    for ln in lines {
        for &v in ln.y {
            if v.is_finite() && (!o.log_y || v > 0.0) {
                ylo = ylo.min(tr(v));
                yhi = yhi.max(tr(v));
            }
        }
    }
    if !ylo.is_finite() {
        ylo = 0.0;
        yhi = 1.0;
    }
    if (yhi - ylo).abs() < 1e-300 {
        yhi = ylo + 1.0;
    }
    let pad = 0.05 * (yhi - ylo);
    let (ylo, yhi) = (ylo - pad, yhi + pad);
    let xlo = x.iter().cloned().fold(f64::INFINITY, f64::min);
    let xhi = x
        .iter()
        .cloned()
        .fold(f64::NEG_INFINITY, f64::max)
        .max(xlo + 1e-300);
    let px = |xv: f64| l + ((xv - xlo) / (xhi - xlo)) as f32 * (r - l);
    let py = |yv: f64| b - ((tr(yv) - ylo) / (yhi - ylo)) as f32 * (b - t);
    let rule = o.theme.rule();
    let ink3 = o.theme.ink3();
    let ic = [ink3[0], ink3[1], ink3[2], 255];
    for tk in nice_ticks(xlo, xhi, 6) {
        let xp = px(tk);
        let mut pb = PathBuilder::new();
        pb.move_to(xp, t);
        pb.line_to(xp, b);
        if let Some(p) = pb.finish() {
            pm.stroke_path(
                &p,
                &paint(rule, 255),
                &Stroke {
                    width: 1.0,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
        }
        text::draw(
            &mut pm,
            Face::Mono,
            12.0,
            xp,
            b + 18.0,
            0.5,
            ic,
            &fmt_num(tk as f32),
        );
    }
    for tk in nice_ticks(ylo, yhi, 5) {
        let yp = b - ((tk - ylo) / (yhi - ylo)) as f32 * (b - t);
        let mut pb = PathBuilder::new();
        pb.move_to(l, yp);
        pb.line_to(r, yp);
        if let Some(p) = pb.finish() {
            pm.stroke_path(
                &p,
                &paint(rule, 255),
                &Stroke {
                    width: 1.0,
                    ..Default::default()
                },
                Transform::identity(),
                None,
            );
        }
        let lab = if o.log_y {
            format!("1e{tk:.0}")
        } else {
            fmt_num(tk as f32)
        };
        text::draw(&mut pm, Face::Mono, 12.0, l - 8.0, yp + 4.0, 1.0, ic, &lab);
    }
    for (i, ln) in lines.iter().enumerate() {
        let c = series_color(i, o.theme);
        let mut pb = PathBuilder::new();
        let mut started = false;
        for (xv, yv) in x.iter().zip(ln.y) {
            if !yv.is_finite() || (o.log_y && *yv <= 0.0) {
                started = false;
                continue;
            }
            let (a, bb) = (px(*xv), py(*yv));
            if started {
                pb.line_to(a, bb)
            } else {
                pb.move_to(a, bb)
            }
            started = true;
        }
        if x.len() == 1 {
            pb.push_circle(px(x[0]), py(ln.y[0]), 3.0);
        }
        if let Some(p) = pb.finish() {
            pm.stroke_path(
                &p,
                &paint(c, 255),
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
            12.0,
            r - 4.0,
            t + 16.0 + 16.0 * i as f32,
            1.0,
            [c[0], c[1], c[2], 255],
            ln.name,
        );
    }
    let ink = o.theme.ink();
    text::draw(
        &mut pm,
        Face::Mono,
        12.0,
        (l + r) / 2.0,
        o.height as f32 - 16.0,
        0.5,
        [ink[0], ink[1], ink[2], 255],
        o.xlabel,
    );
    text::draw(
        &mut pm,
        Face::Mono,
        12.0,
        12.0,
        t - 8.0,
        0.0,
        [ink[0], ink[1], ink[2], 255],
        o.ylabel,
    );
    if let Some(tt) = o.title {
        text::draw(
            &mut pm,
            Face::Sans,
            18.0,
            20.0,
            30.0,
            0.0,
            [ink[0], ink[1], ink[2], 255],
            tt,
        );
    }
    Ok(Png(pm.encode_png()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plot_png() {
        let x: Vec<f64> = (0..50).map(|i| 1500.0 + i as f64).collect();
        let y: Vec<f64> = x.iter().map(|v| (v / 10.0).sin().abs() + 1e-3).collect();
        let png = render_plot(
            &x,
            &[Line { name: "T", y: &y }],
            &PlotOptions {
                width: 600,
                height: 360,
                theme: Theme { dark: true },
                title: Some("Spectrum"),
                xlabel: "wavelength (nm)",
                ylabel: "T",
                log_y: true,
            },
        )
        .unwrap();
        assert!(png.0.len() > 100);
    }
}
