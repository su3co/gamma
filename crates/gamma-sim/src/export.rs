//! S-parameters in tool-neutral formats: Touchstone (`.sNp`, version 1) and
//! the Lumerical INTERCONNECT text format (`.dat`, "Optical N-Port
//! S-Parameter"), so results can go to any circuit simulator.
//!
//! A run with one mode source gives one column of the S-matrix (the input
//! port's). Touchstone needs the whole matrix: the entries that were not
//! computed are written as 0 and a comment says which column is real. The
//! Lumerical format lists only the entries present.
//!
//! Phase convention, the same as lightsim's own writers so either tool gives
//! the same file: lightsim computes with time dependence exp(-i ω t).
//! Touchstone files are written in exp(+j ω t), the convention scikit-rf,
//! ADS and Spectre assume, so the values are the complex conjugate of
//! lightsim's (a header comment says so). The INTERCONNECT text format uses
//! exp(-i ω t) and is written unchanged.

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

/// A port and mode, as lightsim's `SParameters` keys them (`o1@0`).
pub type PortMode = (String, usize);

/// Output and input of an entry, with the modes its name carries
/// (`o2@0,o1@0`; mode 0 when the name has none).
fn keys(e: &crate::results::SEntry) -> (PortMode, PortMode) {
    let mode = |part: Option<&str>| -> usize {
        part.and_then(|p| p.rsplit_once('@'))
            .and_then(|(_, m)| m.trim().parse().ok())
            .unwrap_or(0)
    };
    let mut it = e.name.split(',');
    let mo = mode(it.next());
    let mi = mode(it.next());
    ((e.output.clone(), mo), (e.input.clone(), mi))
}

/// Ports with their modes in a stable order (natural order of the names:
/// o1, o2, o10; then the mode), as lightsim orders them.
pub fn ports(sp: &SParams) -> Vec<PortMode> {
    let mut p: Vec<PortMode> = sp
        .entries
        .iter()
        .flat_map(|e| {
            let (o, i) = keys(e);
            [o, i]
        })
        .collect();
    p.sort_by_key(|(n, m)| (natural(n), *m));
    p.dedup();
    p
}

fn natural(s: &str) -> (String, u64) {
    let i = s.find(|c: char| c.is_ascii_digit()).unwrap_or(s.len());
    (s[..i].to_string(), s[i..].parse().unwrap_or(0))
}

/// Indices of the frequencies in ascending order (descending wavelength).
fn ascending(sp: &SParams) -> Vec<usize> {
    let mut order: Vec<usize> = (0..sp.wavelengths.len()).collect();
    order.sort_by(|&a, &b| sp.wavelengths[b].partial_cmp(&sp.wavelengths[a]).unwrap());
    order
}

/// Touchstone version 1.1, real-imaginary, frequencies in GHz ascending, laid
/// out as lightsim's `SParameters.to_touchstone` writes it: the phase
/// convention, `! port k = name@mode` and the entries not computed in
/// comments, which lightsim's `from_touchstone` reads back.
pub fn touchstone(sp: &SParams, title: &str) -> String {
    let ports = ports(sp);
    let n = ports.len();
    let idx: BTreeMap<&PortMode, usize> = ports.iter().enumerate().map(|(i, p)| (p, i)).collect();
    let mut computed = vec![vec![false; n]; n];
    let mut at = vec![];
    for e in &sp.entries {
        let (o, i) = keys(e);
        let (r, c) = (idx[&o], idx[&i]);
        computed[r][c] = true;
        at.push((r, c));
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "! S-parameters written by gamma {} from a lightsim run",
        gamma_core::VERSION
    );
    let _ = writeln!(
        out,
        "! phase convention: exp(+j w t) (complex conjugate of lightsim's exp(-i w t) values)"
    );
    for (k, (name, mode)) in ports.iter().enumerate() {
        let _ = writeln!(out, "! port {} = {name}@{mode}", k + 1);
    }
    let missing: Vec<String> = (0..n)
        .flat_map(|i| (0..n).map(move |j| (i, j)))
        .filter(|&(i, j)| !computed[i][j])
        .map(|(i, j)| format!("S{},{}", i + 1, j + 1))
        .collect();
    if !missing.is_empty() {
        let _ = writeln!(out, "! not computed (written as 0): {}", missing.join(" "));
    }
    if !title.is_empty() {
        let _ = writeln!(out, "! title: {title}");
    }
    let _ = writeln!(out, "# GHZ S RI R 50");
    for f in ascending(sp) {
        let ghz = C0 / sp.wavelengths[f] / 1e9;
        let mut m = vec![vec![Complex64::new(0.0, 0.0); n]; n];
        for (k, &(r, c)) in at.iter().enumerate() {
            m[r][c] = complex(sp, k, f).conj(); // exp(+j w t)
        }
        let pair = |c: Complex64| format!("{:.12e} {:.12e}", c.re, c.im);
        if n == 2 {
            // Touchstone 1 lists a two-port as S11 S21 S12 S22
            let _ = writeln!(
                out,
                "{ghz:.12e} {} {} {} {}",
                pair(m[0][0]),
                pair(m[1][0]),
                pair(m[0][1]),
                pair(m[1][1])
            );
        } else if n == 1 {
            let _ = writeln!(out, "{ghz:.12e} {}", pair(m[0][0]));
        } else {
            // one matrix row per group of lines, at most four entries per line
            for (i, row) in m.iter().enumerate() {
                for (c, chunk) in row.chunks(4).enumerate() {
                    let body = chunk.iter().map(|x| pair(*x)).collect::<Vec<_>>().join(" ");
                    if i == 0 && c == 0 {
                        let _ = writeln!(out, "{ghz:.12e} {body}");
                    } else {
                        let _ = writeln!(out, "  {body}");
                    }
                }
            }
        }
    }
    out
}

/// Lumerical INTERCONNECT "Optical N-Port S-Parameter" text file, laid out
/// as lightsim's `SParameters.to_lumerical_dat` writes it: the port
/// positions, then one block per computed entry (inputs in port order, then
/// outputs), modes counted from 1, frequency in Hz with |S| and the
/// unwrapped phase, in lightsim's exp(-i w t) convention.
pub fn lumerical(sp: &SParams, sides: &BTreeMap<String, &'static str>) -> String {
    let ports = ports(sp);
    let mut out = String::new();
    let mut names: Vec<&str> = ports.iter().map(|(n, _)| n.as_str()).collect();
    names.dedup();
    for p in names {
        let side = sides
            .get(p)
            .copied()
            .or_else(|| {
                sp.entries
                    .iter()
                    .find(|e| e.output == p)
                    .map(|e| side_name(e.side.0, e.side.1))
            })
            .unwrap_or("LEFT");
        let _ = writeln!(out, "[\"{p}\",\"{side}\"]");
    }
    let order = ascending(sp);
    let pos = |k: &PortMode| ports.iter().position(|p| p == k).unwrap_or(usize::MAX);
    let mut blocks: Vec<usize> = (0..sp.entries.len()).collect();
    blocks.sort_by_key(|&k| {
        let (o, i) = keys(&sp.entries[k]);
        (pos(&i), pos(&o))
    });
    for k in blocks {
        let ((o, mo), (i, mi)) = keys(&sp.entries[k]);
        let _ = writeln!(
            out,
            "('{o}','TE',{},'{i}',{},'transmission')",
            mo + 1,
            mi + 1
        );
        let _ = writeln!(out, "({},3)", order.len());
        let mut prev: Option<f64> = None;
        for &f in &order {
            let c = complex(sp, k, f);
            let mut ph = c.arg();
            if let Some(p) = prev {
                ph += std::f64::consts::TAU * ((p - ph) / std::f64::consts::TAU).round();
            }
            prev = Some(ph);
            let _ = writeln!(
                out,
                "{:.12e}\t{:.12e}\t{:.12e}",
                C0 / sp.wavelengths[f],
                c.norm(),
                ph
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
        assert!(first[0] < C0 / 1.55e-6 / 1e9); // GHz, ascending: 1.6 um first
        assert_eq!(first.len(), 7);
        // S21 = sqrt(0.49) e^{0.5 i} in exp(-i w t) is on the second row,
        // first pair, conjugated for Touchstone's exp(+j w t)
        let row2: Vec<f64> = data[1]
            .split_whitespace()
            .map(|x| x.parse().unwrap())
            .collect();
        assert!((row2[0] - 0.7 * 0.5f64.cos()).abs() < 1e-9);
        assert!((row2[1] + 0.7 * 0.5f64.sin()).abs() < 1e-9);
        assert!(t.contains("! phase convention: exp(+j w t)"));
        assert!(t.contains("! port 1 = o1@0\n! port 2 = o2@0\n! port 3 = o3@0\n"));
        assert!(t.contains("! not computed (written as 0): S1,2 S1,3 S2,2"));
        assert!(t.contains("# GHZ S RI R 50"));
    }

    #[test]
    fn lumerical_blocks() {
        let l = lumerical(&sp(), &BTreeMap::new());
        assert!(l.starts_with("[\"o1\",\"LEFT\"]\n[\"o2\",\"RIGHT\"]"));
        assert_eq!(l.matches(",'transmission')").count(), 3);
        assert!(l.contains("('o2','TE',1,'o1',1,'transmission')\n(2,3)\n"));
        assert!(l.contains("(2,3)"));
        // INTERCONNECT keeps exp(-i w t): the phase is written unchanged
        let line = l
            .lines()
            .skip_while(|x| !x.starts_with("('o2'"))
            .nth(2)
            .unwrap();
        let phase: f64 = line.split_whitespace().nth(2).unwrap().parse().unwrap();
        assert!((phase - 0.5).abs() < 1e-9);
    }

    #[test]
    fn lumerical_phase_is_unwrapped() {
        let mut s = sp();
        s.entries[1].phase = vec![3.0, -3.0]; // 1.6 um, then 1.5 um
        let l = lumerical(&s, &BTreeMap::new());
        let rows: Vec<f64> = l
            .lines()
            .skip_while(|x| !x.starts_with("('o2'"))
            .skip(2)
            .take(2)
            .map(|x| x.split_whitespace().nth(2).unwrap().parse().unwrap())
            .collect();
        assert!((rows[0] - 3.0).abs() < 1e-12);
        assert!((rows[1] - (std::f64::consts::TAU - 3.0)).abs() < 1e-12);
    }
}
