//! Text on a pixmap with ab_glyph (IBM Plex Mono for numbers and labels,
//! Archivo for titles).

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use tiny_skia::Pixmap;

static MONO: &[u8] = include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf");
static SANS: &[u8] = include_bytes!("../../../assets/fonts/Archivo.ttf");

#[derive(Clone, Copy)]
pub enum Face {
    Mono,
    Sans,
}

fn font(f: Face) -> FontRef<'static> {
    FontRef::try_from_slice(match f {
        Face::Mono => MONO,
        Face::Sans => SANS,
    })
    .expect("bundled font")
}

pub fn width(face: Face, size: f32, s: &str) -> f32 {
    let f = font(face);
    let sf = f.as_scaled(PxScale::from(size));
    s.chars().map(|c| sf.h_advance(f.glyph_id(c))).sum()
}

/// Draw `s` with its baseline at y; `align` 0 = left, 0.5 = centre, 1 = right.
pub fn draw(
    pm: &mut Pixmap,
    face: Face,
    size: f32,
    x: f32,
    y: f32,
    align: f32,
    rgba: [u8; 4],
    s: &str,
) {
    let f = font(face);
    let sf = f.as_scaled(PxScale::from(size));
    let mut cx = x - align * width(face, size, s);
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    let data = pm.data_mut();
    for c in s.chars() {
        let id = f.glyph_id(c);
        let g = id.with_scale_and_position(PxScale::from(size), point(cx, y));
        cx += sf.h_advance(id);
        if let Some(og) = f.outline_glyph(g) {
            let b = og.px_bounds();
            og.draw(|gx, gy, cov| {
                let px = b.min.x as i32 + gx as i32;
                let py = b.min.y as i32 + gy as i32;
                if px < 0 || py < 0 || px >= w || py >= h {
                    return;
                }
                let i = ((py * w + px) * 4) as usize;
                let a = cov * rgba[3] as f32 / 255.0;
                for k in 0..3 {
                    // premultiplied blend
                    let dst = data[i + k] as f32;
                    data[i + k] = (rgba[k] as f32 * a + dst * (1.0 - a)).round() as u8;
                }
                let da = data[i + 3] as f32;
                data[i + 3] = (255.0 * a + da * (1.0 - a)).round().min(255.0) as u8;
            });
        }
    }
}
