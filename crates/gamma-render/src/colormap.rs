//! Colour maps for permittivity, intensity and signed fields, per theme.

pub type Rgb = [u8; 3];

fn lerp(stops: &[(f32, Rgb)], t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    for w in stops.windows(2) {
        let (t0, c0) = w[0];
        let (t1, c1) = w[1];
        if t <= t1 {
            let u = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            return [0, 1, 2]
                .map(|k| (c0[k] as f32 + u * (c1[k] as f32 - c0[k] as f32)).round() as u8);
        }
    }
    stops.last().unwrap().1
}

/// Intensity (|E|^2), from the ground of the theme to a bright end.
pub fn intensity(t: f32, dark: bool) -> Rgb {
    if dark {
        lerp(
            &[
                (0.0, [12, 14, 18]),
                (0.25, [58, 33, 110]),
                (0.55, [176, 58, 91]),
                (0.8, [240, 140, 60]),
                (1.0, [252, 236, 170]),
            ],
            t,
        )
    } else {
        lerp(
            &[
                (0.0, [255, 255, 255]),
                (0.2, [230, 205, 160]),
                (0.5, [214, 110, 70]),
                (0.8, [110, 40, 110]),
                (1.0, [22, 25, 31]),
            ],
            t,
        )
    }
}

/// Signed field (Re E), t in [-1, 1].
pub fn diverging(t: f32, dark: bool) -> Rgb {
    let mid: Rgb = if dark { [12, 14, 18] } else { [255, 255, 255] };
    lerp(
        &[
            (0.0, [64, 120, 220]),
            (0.25, [120, 170, 235]),
            (0.5, mid),
            (0.75, [240, 150, 100]),
            (1.0, [214, 80, 40]),
        ],
        0.5 * (t + 1.0),
    )
}

/// Permittivity, from low (ground) to high (ink).
pub fn material(t: f32, dark: bool) -> Rgb {
    if dark {
        lerp(&[(0.0, [21, 25, 32]), (1.0, [214, 200, 160])], t)
    } else {
        lerp(&[(0.0, [244, 245, 247]), (1.0, [60, 66, 80])], t)
    }
}

pub fn hex(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}
