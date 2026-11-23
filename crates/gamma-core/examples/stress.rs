//! Large-layout timing: writes a GDS with N rectangles (a grating-like array
//! in cells, flattened) and times reading, flattening, triangulating the
//! viewer buffers and a pick. `cargo run --release -p gamma-core --example stress -- 1000000`
use std::time::Instant;

fn main() {
    let n: usize = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);
    let per = 1000usize; // rectangles per unit cell
    let cells = n.div_ceil(per);
    let mut w = gamma_core::gds::Writer::new("stress", 1e-9);
    w.begin_cell("unit");
    for i in 0..per {
        let x = i as f64 * 0.6;
        w.polygon(
            (1, 0),
            &[[x, 0.0], [x + 0.3, 0.0], [x + 0.3, 2.0], [x, 2.0]],
        );
    }
    w.end_cell();
    w.begin_cell("top");
    for j in 0..cells {
        w.sref("unit", [0.0, j as f64 * 3.0], 0.0, false);
    }
    w.end_cell();
    let path = std::env::temp_dir().join(format!("gamma-stress-{}.gds", std::process::id()));
    std::fs::write(&path, w.finish()).unwrap();
    let size = std::fs::metadata(&path).unwrap().len();
    let t = Instant::now();
    let l = gamma_core::Layout::read(&path, Some("top"), None).unwrap();
    let t_read = t.elapsed();
    let t = Instant::now();
    let mesh = l.layer_mesh_bytes((1, 0)).unwrap();
    let t_mesh = t.elapsed();
    let t = Instant::now();
    let hit = l.pick([300.15, 1.0], 0.01, None);
    let t_pick = t.elapsed();
    println!(
        "{} polygons ({} kB GDS): read+flatten {:.2} s, viewer triangles {:.2} s ({} MB), pick {:.1} ms ({})",
        l.polygon_count(),
        size / 1024,
        t_read.as_secs_f64(),
        t_mesh.as_secs_f64(),
        mesh.len() / 1_000_000,
        t_pick.as_secs_f64() * 1e3,
        hit.map(|h| h.reference).unwrap_or_default()
    );
    let _ = std::fs::remove_file(path);
}
