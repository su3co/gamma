//! Draws the provisional app icon (a bold lowercase gamma in Literata on the
//! dark ground; Archivo has no Greek letters): `cargo run -p gamma-render --example icon -- out.png`.
use ab_glyph::{Font, FontRef, PxScale, VariableFont, point};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| "icon.png".into());
    let n = 1024u32;
    let mut pm = Pixmap::new(n, n).unwrap();
    // rounded square (macOS grid: 824 px body with 100 px margins)
    let (m, r, s) = (100.0f32, 185.0f32, 824.0f32);
    let mut pb = PathBuilder::new();
    pb.move_to(m + r, m);
    pb.line_to(m + s - r, m);
    pb.quad_to(m + s, m, m + s, m + r);
    pb.line_to(m + s, m + s - r);
    pb.quad_to(m + s, m + s, m + s - r, m + s);
    pb.line_to(m + r, m + s);
    pb.quad_to(m, m + s, m, m + s - r);
    pb.line_to(m, m + r);
    pb.quad_to(m, m, m + r, m);
    pb.close();
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(12, 14, 18, 255));
    paint.anti_alias = true;
    pm.fill_path(&pb.finish().unwrap(), &paint, FillRule::Winding, Transform::identity(), None);
    let mut font = FontRef::try_from_slice(include_bytes!("../../../assets/fonts/Literata.ttf")).unwrap();
    font.set_variation(b"wght", 700.0);
    font.set_variation(b"opsz", 72.0);
    let size = 760.0;
    let g = font.glyph_id('γ').with_scale_and_position(PxScale::from(size), point(0.0, 0.0));
    let og = font.outline_glyph(g).unwrap();
    let b = og.px_bounds();
    let (w, h) = (b.width(), b.height());
    let ox = (n as f32 - w) / 2.0 - b.min.x;
    let oy = (n as f32 - h) / 2.0 - b.min.y + 10.0;
    let data = pm.data_mut();
    og.draw(|x, y, c| {
        let px = (x as f32 + b.min.x + ox) as i32;
        let py = (y as f32 + b.min.y + oy) as i32;
        if px < 0 || py < 0 || px >= n as i32 || py >= n as i32 {
            return;
        }
        let i = ((py as u32 * n + px as u32) * 4) as usize;
        let ink = [236.0, 238.0, 242.0];
        for k in 0..3 {
            data[i + k] = (ink[k] * c + data[i + k] as f32 * (1.0 - c)) as u8;
        }
    });
    pm.save_png(&out).unwrap();
    println!("wrote {out}");
}
