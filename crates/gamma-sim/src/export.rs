//! S-parameters in tool-neutral formats: Touchstone (`.sNp`, version 1) and
//! the Lumerical INTERCONNECT text format (`.dat`, "Optical N-Port
//! S-Parameter"), so results can go to any circuit simulator.
//!
//! A run with one mode source gives one column of the S-matrix (the input
//! port's). Touchstone needs the whole matrix: the entries that were not
//! computed are written as 0 and a comment says which column is real. The
//! Lumerical format lists only the entries present.

use crate::results::SParams;
use lightsim_core::constants::C0;
use num_complex::Complex64;
use std::collections::BTreeMap;
use std::fmt::Write;

/// Side of a port as INTERCONNECT draws it.
pub fn side_name(axis: usize, side: i8) -> &'static str {
    match (axis, side) {
        (0, s) if s < 0 => "LEFT",
        (0, _) => "RIGHT",
        (1, s) if s < 0 => "BOTTOM",
        _ => "TOP",
    }
}

fn complex(sp: &SParams, entry: usize, f: usize) -> Complex64 {
    let e = &sp.entries[entry];
    Complex64::from_polar(e.abs2[f].max(0.0).sqrt(), e.phase[f])
}

/// Ports in a stable order (natural order of their names: o1, o2, o10).
pub fn ports(sp: &SParams) -> Vec<String> {
    let mut p: Vec<String> = sp
        .entries
        .iter()
        .flat_map(|e| [e.output.clone(), e.input.clone()])
        .collect();
    p.sort_by_key(|a| natural(a));
    p.dedup();
    p
}

fn natural(s: &str) -> (String, u64) {
    let i = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    (s[..i].to_string(), s[i..].parse().unwrap_or(0))
}

/// Touchstone version 1, real-imaginary, frequencies in Hz ascending.
pub fn touchstone(sp: &SParams, title: &str) -> String {
    let ports = ports(sp);
    let n = ports.len();
    let idx: BTreeMap<&str, usize> = ports
        .iter()
        .enumerate()
        .map(|(i, p)| (p.as_str(), i))
        .collect();
    let mut order: Vec<usize> = (0..sp.wavelengths.len()).collect();
    order.sort_by(|&a, &b| sp.wavelengths[b].partial_cmp(&sp.wavelengths[a]).unwrap()); // ascending frequency
    let mut out = String::new();
    let _ = writeln!(out, "! {title}");
    let _ = writeln!(
        out,
        "! written by gamma {} from a lightsim run",
        gamma_core::VERSION
    );
    let _ = writeln!(
        out,
        "! ports: {}",
        ports
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}={p}", i + 1))
            .collect::<Vec<_>>()
            .join(" ")
    );
    let inputs: Vec<&str> = {
        let mut v: Vec<&str> = sp.entries.iter().map(|e| e.input.as_str()).collect();
        v.dedup();
        v
    };
    let _ = writeln!(
        out,
        "! computed columns (inputs): {}; other entries are 0",
        inputs.join(", ")
    );
    let _ = writeln!(out, "# Hz S RI R 50");
    for &f in &order {
        let freq = C0 / sp.wavelengths[f];
        let mut m = vec![vec![Complex64::new(0.0, 0.0); n]; n];
        for (k, e) in sp.entries.iter().enumerate() {
            if let (Some(&i), Some(&j)) = (idx.get(e.output.as_str()), idx.get(e.input.as_str())) {
                m[i][j] = complex(sp, k, f);
            }
        }
        let pair = |c: Complex64| format!("{:.9e} {:.9e}", c.re, c.im);
        if n == 2 {
            // two-port files list S11 S21 S12 S22
            let _ = writeln!(
                out,
                "{freq:.9e} {} {} {} {}",
                pair(m[0][0]),
                pair(m[1][0]),
                pair(m[0][1]),
                pair(m[1][1])
            );
        } else if n == 1 {
            let _ = writeln!(out, "{freq:.9e} {}", pair(m[0][0]));
        } else {
            // one matrix row per line group, at most four pairs per line
            for (i, row) in m.iter().enumerate() {
                for (c, chunk) in row.chunks(4).enumerate() {
                    let body = chunk.iter().map(|x| pair(*x)).collect::<Vec<_>>().join(" ");
                    if i == 0 && c == 0 {
                        let _ = writeln!(out, "{freq:.9e} {body}");
                    } else {
                        let _ = writeln!(out, "{:>16} {body}", "");
                    }
                }
            }
        }
    }
    out
}

/// Lumerical INTERCONNECT "Optical N-Port S-Parameter" text file.
pub fn lumerical(sp: &SParams, sides: &BTreeMap<String, &'static str>) -> String {
    let ports = ports(sp);
    let mut out = String::new();
    for p in &ports {
        let side = sides
            .get(p)
            .copied()
            .or_else(|| {
                sp.entries
                    .iter()
                    .find(|e| &e.output == p)
                    .map(|e| side_name(e.side.0, e.side.1))
            })
            .unwrap_or("LEFT");
        let _ = writeln!(out, "[\"{p}\",\"{side}\"]");
    }
    let mut order: Vec<usize> = (0..sp.wavelengths.len()).collect();
    order.sort_by(|&a, &b| sp.wavelengths[b].partial_cmp(&sp.wavelengths[a]).unwrap());
    for (k, e) in sp.entries.iter().enumerate() {
        let _ = writeln!(
            out,
            "(\"{}\",\"mode 1\",1,\"{}\",1,\"transmission\")",
            e.output, e.input
        );
        let _ = writeln!(out, "({},3)", order.len());
        for &f in &order {
            let c = complex(sp, k, f);
            let _ = writeln!(
                out,
                "{:.9e} {:.9e} {:.9e}",
                C0 / sp.wavelengths[f],
                c.norm(),
                c.arg()
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::results::{SEntry, SParams};

    fn sp() -> SParams {
        let e = |o: &str, a2: f64, ph: f64, side: (usize, i8)| SEntry {
            name: format!("{o}@0,o1@0"),
            db: vec![10.0 * a2.log10(); 2],
            phase: vec![ph; 2],
            abs2: vec![a2; 2],
            output: o.into(),
            input: "o1".into(),
            monitor: format!("m_{o}"),
            side,
        };
        SParams {
            input: "o1".into(),
            wavelengths: vec![1.6e-6, 1.5e-6],
            entries: vec![
                e("o1", 0.001, 0.0, (0, -1)),
                e("o2", 0.49, 0.5, (0, 1)),
                e("o3", 0.49, 0.5, (0, 1)),
            ],
            note: String::new(),
        }
    }

    #[test]
    fn touchstone_three_port() {
        let t = touchstone(&sp(), "mmi");
        let data: Vec<&str> = t
            .lines()
            .filter(|l| !l.starts_with('!') && !l.starts_with('#'))
            .collect();
        // 2 frequencies x 3 rows
        assert_eq!(data.len(), 6);
        let first: Vec<f64> = data[0]
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        assert!(first[0] < C0 / 1.55e-6); // ascending frequency: 1.6 um first
        assert_eq!(first.len(), 7);
        // S21 = sqrt(0.49) e^{0.5 i} is on the second row, first pair
        let row2: Vec<f64> = data[1]
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        assert!((row2[0] - 0.7 * 0.5f64.cos()).abs() < 1e-9);
        assert!(t.contains("# Hz S RI R 50"));
    }

    #[test]
    fn lumerical_blocks() {
        let l = lumerical(&sp(), &BTreeMap::new());
        assert!(l.starts_with("[\"o1\",\"LEFT\"]\n[\"o2\",\"RIGHT\"]"));
        assert_eq!(l.matches("\"transmission\")").count(), 3);
        assert!(l.contains("(2,3)"));
    }
}
