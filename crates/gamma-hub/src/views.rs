//! Rendering of any viewer to PNG for agents and the CLI.

use crate::projects::ProjectState;
use anyhow::{Context, Result, bail};
use gamma_core::geom::BBox;
use gamma_core::refs::ObjRef;
use gamma_render::colormap::Rgb;
use gamma_render::plot::{Line, PlotOptions, render_plot};
use gamma_render::scene::{
    LayoutOptions, MapKind, MapOptions, Overlay, Theme, render_field, render_layout, render_stack3d,
};
use std::collections::HashMap;

pub type Params = HashMap<String, String>;

fn num(p: &Params, k: &str, d: f64) -> f64 {
    p.get(k).and_then(|v| v.parse().ok()).unwrap_or(d)
}

fn latest_run(ps: &ProjectState, sim: &str) -> Result<String> {
    ps.runs(Some(sim))
        .into_iter()
        .find(|r| r.has_results)
        .map(|r| r.id)
        .with_context(|| format!("simulation {sim} has no finished run yet"))
}

/// Render `view` with parameters `p`. Views: layout, 3d, setup, field,
/// spectrum, sparams, decay, time, live.
pub fn render(
    ps: &ProjectState,
    jobs: &crate::jobs::Jobs,
    view: &str,
    p: &Params,
) -> Result<Vec<u8>> {
    let w = num(p, "width", 1200.0) as u32;
    let h = num(p, "height", 800.0) as u32;
    let theme = Theme::parse(p.get("theme").map(|s| s.as_str()).unwrap_or("dark"));
    let accent: Rgb = theme.accent();
    match view {
        "layout" => {
            let l = ps
                .layout()?
                .context("the project has no layout yet; build it first")?;
            let stack = ps.stack();
            let mut highlight = vec![];
            if let Some(r) = p.get("highlight") {
                for s in r.split(';') {
                    if let Some(b) = ObjRef::parse(s).and_then(|o| l.bbox_of(&o)) {
                        highlight.push(b);
                    }
                }
            }
            let window = p.get("window").and_then(|s| {
                let v: Vec<f64> = s.split(',').filter_map(|x| x.parse().ok()).collect();
                (v.len() == 4).then(|| BBox {
                    min: [v[0], v[1]],
                    max: [v[2], v[3]],
                })
            });
            let png = render_layout(
                &l,
                stack.as_deref(),
                &LayoutOptions {
                    width: w,
                    height: h,
                    theme,
                    window,
                    highlight,
                    hidden_layers: vec![],
                    title: Some(format!(
                        "{} — {}",
                        ps.snapshot().manifest.project.name,
                        l.top
                    )),
                },
            )?;
            Ok(png.0)
        }
        "3d" => {
            let l = ps.layout()?.context("the project has no layout yet")?;
            let stack = ps.stack().context("the project has no layer stack")?;
            Ok(render_stack3d(
                &l,
                &stack,
                w,
                h,
                theme,
                Some(&format!("{} — 3-D", ps.snapshot().manifest.project.name)),
            )?
            .0)
        }
        "setup" => {
            let sim_name = p.get("sim").context("setup view needs sim=<name>")?;
            let preview = p
                .get("preview")
                .map(|v| v == "1" || v == "true")
                .unwrap_or(false);
            let sim = ps.sim(sim_name, preview)?;
            let axis = num(p, "axis", 2.0) as usize;
            let coord = match p.get("coord").and_then(|c| c.parse::<f64>().ok()) {
                Some(c) => c,
                None => gamma_sim::runner::SliceRequest::default_for(&sim).coord,
            };
            let sl = gamma_sim::setup::eps_slice(&sim, axis.min(2), coord, 1600)?;
            let summary = gamma_sim::setup::summarize(sim_name, &sim, &[])?;
            let (a0, a1) = (sl.axes[0], sl.axes[1]);
            let mut overlays = vec![];
            for o in summary.objects.iter().filter(|o| o.kind != "structure") {
                // objects that cross the plane
                if o.min[axis] > sl.coord + 1e-12 || o.max[axis] < sl.coord - 1e-12 {
                    continue;
                }
                let color: Rgb = if o.kind == "source" {
                    accent
                } else if theme.dark {
                    [79, 209, 181]
                } else {
                    [15, 123, 108]
                };
                overlays.push(Overlay {
                    label: o.name.clone(),
                    min: [o.min[a0], o.min[a1]],
                    max: [o.max[a0], o.max[a1]],
                    color,
                });
            }
            // the region inside the absorbing layers
            overlays.push(Overlay {
                label: "region".into(),
                min: [summary.region[0][a0], summary.region[0][a1]],
                max: [summary.region[1][a0], summary.region[1][a1]],
                color: theme.ink3(),
            });
            let names = ["x", "y", "z"];
            let title = format!(
                "{sim_name}: permittivity at {} = {:.3} µm{}",
                names[axis],
                sl.coord * 1e6,
                if preview { " (preview mesh)" } else { "" }
            );
            Ok(render_field(
                &sl.u,
                &sl.v,
                &sl.eps,
                &MapOptions {
                    width: w,
                    height: h,
                    theme,
                    kind: MapKind::Material,
                    title: Some(&title),
                    axis_names: [names[a0], names[a1]],
                    overlays,
                    colorbar_label: "eps",
                },
            )?
            .0)
        }
        "field" => {
            let sim_name = p.get("sim").context("field view needs sim=<name>")?;
            let run = match p.get("run") {
                Some(r) => r.clone(),
                None => latest_run(ps, sim_name)?,
            };
            let monitor = p
                .get("monitor")
                .context("field view needs monitor=<name>")?;
            let q = p.get("quantity").map(|s| s.as_str()).unwrap_or("|E|2");
            let fi = num(p, "f", 0.0) as usize;
            let spec = ps.run_spec(sim_name, &run)?;
            let l = gamma_sim::results::Loaded::read(
                &ps.run_dir(sim_name, &run)?.join("results.json"),
                &spec,
            )?;
            let fm = gamma_sim::results::field_map(&l, monitor, q, fi)?;
            let names = ["x", "y", "z"];
            let kind = if q.starts_with('|') || q.starts_with("abs") {
                MapKind::Intensity
            } else {
                MapKind::Signed
            };
            let title = format!("{sim_name}/{monitor}: {q} at {:.1} nm", fm.wavelength * 1e9);
            Ok(render_field(
                &fm.u,
                &fm.v,
                &fm.data,
                &MapOptions {
                    width: w,
                    height: h,
                    theme,
                    kind,
                    title: Some(&title),
                    axis_names: [names[fm.axes[0]], names[fm.axes[1]]],
                    overlays: vec![],
                    colorbar_label: q,
                },
            )?
            .0)
        }
        "live" => {
            let job = p.get("job").context("live view needs job=<id>")?;
            let s = jobs
                .last_slice(job)
                .context("no field slice from this job yet")?;
            let s: gamma_sim::stream::Slice = serde_json::from_value(s)?;
            let vals = s.values();
            let u: Vec<f64> = (0..s.dims[0]).map(|i| i as f64 * 1e-6).collect();
            let v: Vec<f64> = (0..s.dims[1]).map(|i| i as f64 * 1e-6).collect();
            let title = format!("{} at step {} (grid cells)", s.comp, s.step);
            Ok(render_field(
                &u,
                &v,
                &vals,
                &MapOptions {
                    width: w,
                    height: h,
                    theme,
                    kind: MapKind::Signed,
                    title: Some(&title),
                    axis_names: ["i", "j"],
                    overlays: vec![],
                    colorbar_label: &s.comp,
                },
            )?
            .0)
        }
        "spectrum" | "sparams" | "decay" | "time" => {
            let sim_name = p.get("sim").context("needs sim=<name>")?;
            let run = match p.get("run") {
                Some(r) => r.clone(),
                None => latest_run(ps, sim_name)?,
            };
            let spec = ps.run_spec(sim_name, &run)?;
            let l = gamma_sim::results::Loaded::read(
                &ps.run_dir(sim_name, &run)?.join("results.json"),
                &spec,
            )?;
            match view {
                "sparams" => {
                    let sp = gamma_sim::results::sparams(&l)?.context(
                        "this run has no S-parameters (needs one mode source and mode monitors)",
                    )?;
                    let x: Vec<f64> = sp.wavelengths.iter().map(|w| w * 1e9).collect();
                    let lines: Vec<Line> = sp
                        .entries
                        .iter()
                        .map(|e| Line {
                            name: &e.name,
                            y: &e.db,
                        })
                        .collect();
                    Ok(render_plot(
                        &x,
                        &lines,
                        &PlotOptions {
                            width: w,
                            height: h,
                            theme,
                            title: Some(&format!("{sim_name}: S-parameters")),
                            xlabel: "wavelength (nm)",
                            ylabel: "|S|² (dB)",
                            log_y: false,
                        },
                    )?
                    .0)
                }
                "spectrum" => {
                    let monitor = p
                        .get("monitor")
                        .context("spectrum view needs monitor=<name>")?;
                    let s = gamma_sim::results::spectrum(&l, monitor)?;
                    let x: Vec<f64> = s.wavelengths.iter().map(|w| w * 1e9).collect();
                    let lines: Vec<Line> = s
                        .series
                        .iter()
                        .map(|e| Line {
                            name: &e.name,
                            y: &e.values,
                        })
                        .collect();
                    Ok(render_plot(
                        &x,
                        &lines,
                        &PlotOptions {
                            width: w,
                            height: h,
                            theme,
                            title: Some(&format!("{sim_name}/{monitor}")),
                            xlabel: "wavelength (nm)",
                            ylabel: &s.unit,
                            log_y: false,
                        },
                    )?
                    .0)
                }
                "decay" => {
                    let t = gamma_sim::results::energy_decay(&l);
                    let x: Vec<f64> = t.times.iter().map(|v| v * 1e15).collect();
                    let lines: Vec<Line> = t
                        .series
                        .iter()
                        .map(|e| Line {
                            name: &e.name,
                            y: &e.values,
                        })
                        .collect();
                    Ok(render_plot(
                        &x,
                        &lines,
                        &PlotOptions {
                            width: w,
                            height: h,
                            theme,
                            title: Some(&format!("{sim_name}: field energy decay")),
                            xlabel: "time (fs)",
                            ylabel: "energy / peak",
                            log_y: true,
                        },
                    )?
                    .0)
                }
                _ => {
                    let monitor = p.get("monitor").context("time view needs monitor=<name>")?;
                    let t = gamma_sim::results::time_series(&l, monitor)?;
                    let x: Vec<f64> = t.times.iter().map(|v| v * 1e15).collect();
                    let lines: Vec<Line> = t
                        .series
                        .iter()
                        .map(|e| Line {
                            name: &e.name,
                            y: &e.values,
                        })
                        .collect();
                    Ok(render_plot(
                        &x,
                        &lines,
                        &PlotOptions {
                            width: w,
                            height: h,
                            theme,
                            title: Some(&format!("{sim_name}/{monitor}")),
                            xlabel: "time (fs)",
                            ylabel: "field",
                            log_y: false,
                        },
                    )?
                    .0)
                }
            }
        }
        other => bail!(
            "unknown view '{other}': layout, 3d, setup, field, live, spectrum, sparams, decay, time"
        ),
    }
}
