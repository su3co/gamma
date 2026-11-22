//! Running a simulation with a live view: progress, energy decay and field
//! slices about once a second, and cancellation.
//!
//! Single-device runs step lightsim's backend directly through its public
//! [`Backend`] trait (the same order of operations as lightsim's driver,
//! `lightsim_core::run`), which lets the runner read a field plane between
//! steps. Several GPUs use lightsim's multi-slab driver, whose progress lines
//! the worker forwards.

use crate::devices::Device;
use crate::stream::{Msg, Slice};
use anyhow::{Result, bail};
use lightsim_core::engine::{Backend, cpu::CpuBackend};
use lightsim_core::grid::Component;
use lightsim_core::plan::{CompileOptions, compile};
use lightsim_core::results::{RunStats, assemble, finalize};
use lightsim_core::{Results, Simulation};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// Which field plane to stream: component, normal axis and coordinate (m).
#[derive(Clone, Debug)]
pub struct SliceRequest {
    pub comp: Component,
    pub axis: usize,
    pub coord: f64,
}

impl SliceRequest {
    /// The plane through the middle of the structures (or region), normal to z,
    /// of the component the first source drives (else Ey).
    pub fn default_for(sim: &Simulation) -> SliceRequest {
        use lightsim_core::sources::Source as S;
        let comp = sim
            .sources
            .iter()
            .find_map(|s| match s {
                S::PointDipole { polarization, .. }
                | S::PlaneWave { polarization, .. }
                | S::GaussianBeam { polarization, .. }
                | S::Tfsf { polarization, .. } => Some(*polarization),
                S::Mode { axis, .. } => Some(if *axis == 0 {
                    Component::Ey
                } else {
                    Component::Ex
                }),
            })
            .unwrap_or(Component::Ey);
        // z through the middle of the polygon slabs if any
        let mut zs = vec![];
        for st in &sim.structures {
            if let lightsim_core::geometry::Shape::PolySlab {
                slab_min,
                slab_max,
                axis: 2,
                ..
            } = &st.shape
            {
                zs.push(0.5 * (slab_min + slab_max));
            }
        }
        let coord = zs
            .first()
            .copied()
            .unwrap_or(0.5 * (sim.region.min[2] + sim.region.max[2]));
        let axis = if sim.dimensions == 2 {
            sim.invariant_axis
        } else {
            2
        };
        SliceRequest { comp, axis, coord }
    }
}

pub struct RunConfig {
    pub device: Device,
    pub slice: Option<SliceRequest>,
    /// Seconds between progress messages (and slices).
    pub every_s: f64,
}

/// Run `sim`. `emit` receives progress messages; `cancel` stops the run
/// (results accumulated so far are returned with `stopped_early`).
pub fn run(
    sim: &Simulation,
    cfg: &RunConfig,
    cancel: Arc<AtomicBool>,
    emit: &mut (dyn FnMut(Msg) + Send),
) -> Result<Results> {
    sim.validate()?;
    match &cfg.device {
        Device::Cpu => {
            let plan = compile(sim, &CompileOptions::default())?;
            emit(Msg::Plan {
                cells: plan.grid.num_cells(),
                steps: plan.nsteps,
                dt: plan.dt,
                memory_bytes: plan.memory_bytes() as u64,
                device: "cpu".into(),
            });
            let be = CpuBackend::new(plan)?;
            drive(Box::new(be), sim, cfg, cancel, emit)
        }
        Device::Cuda { gpus } if gpus.len() == 1 => {
            #[cfg(feature = "cuda")]
            {
                let plan = compile(sim, &CompileOptions::default())?;
                emit(Msg::Plan {
                    cells: plan.grid.num_cells(),
                    steps: plan.nsteps,
                    dt: plan.dt,
                    memory_bytes: plan.memory_bytes() as u64,
                    device: cfg.device.label(),
                });
                let be = lightsim_cuda::CudaBackend::new(plan, gpus[0])?;
                drive(Box::new(be), sim, cfg, cancel, emit)
            }
            #[cfg(not(feature = "cuda"))]
            {
                let _ = gpus;
                bail!("this gamma was built without CUDA; run on the CPU or on a remote GPU host")
            }
        }
        Device::Cuda { gpus } => {
            #[cfg(feature = "cuda")]
            {
                let grid = sim.build_grid()?;
                let dt = sim.time_step(&grid)?;
                emit(Msg::Plan {
                    cells: grid.num_cells(),
                    steps: (sim.run_time / dt).ceil() as usize,
                    dt,
                    memory_bytes: crate::setup::memory_estimate(sim, &grid),
                    device: cfg.device.label(),
                });
                let opts = lightsim_core::run::RunOptions {
                    progress_every: 200,
                };
                lightsim_cuda::multi::run_multi_gpu(sim, gpus, &opts)
            }
            #[cfg(not(feature = "cuda"))]
            {
                let _ = gpus;
                bail!("this gamma was built without CUDA; run on the CPU or on a remote GPU host")
            }
        }
    }
}

fn drive(
    mut be: Box<dyn Backend + '_>,
    sim: &Simulation,
    cfg: &RunConfig,
    cancel: Arc<AtomicBool>,
    emit: &mut (dyn FnMut(Msg) + Send),
) -> Result<Results> {
    if be.on_thread_pool() && rayon::current_thread_index().is_none() {
        // CPU backends step from a thread of the pool they compute on
        let mut out = None;
        rayon::scope(|_| out = Some(drive_loop(&mut be, sim, cfg, &cancel, emit)));
        return out.unwrap();
    }
    drive_loop(&mut be, sim, cfg, &cancel, emit)
}

fn drive_loop(
    be: &mut Box<dyn Backend + '_>,
    sim: &Simulation,
    cfg: &RunConfig,
    cancel: &AtomicBool,
    emit: &mut (dyn FnMut(Msg) + Send),
) -> Result<Results> {
    let p0 = be.plan().clone();
    let nsteps = p0.nsteps;
    let interval = p0.shutoff_interval.max(1);
    let t_src_end = p0.source_end_time;
    let total_cells = p0.grid.num_cells();
    let slice_idx = cfg.slice.as_ref().map(|s| {
        (
            s.comp,
            s.axis.min(2),
            p0.grid
                .cell_index(s.axis.min(2), s.coord)
                .min(p0.n[s.axis.min(2)] - 1),
        )
    });
    let t0 = Instant::now();
    let (mut max_energy, mut source_peak) = (0.0f64, 0.0f64);
    let mut steps_done = nsteps;
    let mut stopped = false;
    let mut energy_steps = vec![];
    let mut energy_trace = vec![];
    let mut next_report = cfg.every_s.min(0.5);
    let mut decay: Option<f64> = None;
    for n in 0..nsteps {
        be.fill_ghost_e()?;
        be.update_h()?;
        be.inject_h(n)?;
        be.fill_ghost_h()?;
        be.update_e()?;
        be.finish_e()?;
        be.inject_e(n)?;
        be.record(n)?;
        if (n + 1) % interval == 0 || n + 1 == nsteps {
            let en = be.energy()?;
            let t = (n + 1) as f64 * p0.dt;
            let runaway = t > t_src_end && source_peak > 0.0 && en > 1e3 * source_peak;
            if !en.is_finite() || runaway {
                bail!(
                    "the simulation is unstable: the field energy {} at step {} of {}. Results are not returned. Usual causes: a dispersive medium whose poles are too strong for the time step, a medium with eps < 1 or negative conductivity, or courant > 1.",
                    if en.is_finite() {
                        format!(
                            "has grown to {:.1e} times its value when the sources stopped",
                            en / source_peak
                        )
                    } else {
                        "is not finite".to_string()
                    },
                    n + 1,
                    nsteps
                );
            }
            energy_steps.push(n + 1);
            energy_trace.push(en);
            if t <= t_src_end || source_peak == 0.0 {
                source_peak = source_peak.max(en);
            }
            max_energy = max_energy.max(en);
            if max_energy > 0.0 {
                decay = Some(en / max_energy);
            }
            if let Some(th) = p0.shutoff
                && (n + 1) % interval == 0
                && t > t_src_end
                && en <= th * max_energy
            {
                steps_done = n + 1;
                stopped = true;
                break;
            }
        }
        if cancel.load(Ordering::Relaxed) {
            steps_done = n + 1;
            stopped = true;
            break;
        }
        let el = t0.elapsed().as_secs_f64();
        if el >= next_report || n + 1 == nsteps {
            let rate = total_cells as f64 * (n + 1) as f64 / el.max(1e-9) / 1e6;
            emit(Msg::Progress {
                step: n + 1,
                steps: nsteps,
                elapsed: el,
                decay,
                mcells_per_s: rate,
            });
            if let Some((comp, axis, idx)) = slice_idx {
                be.synchronize()?;
                let f = be.field(comp)?;
                let (dims, data) = plane(&f, p0.n, axis, idx);
                emit(Msg::Slice(Slice::new(
                    n + 1,
                    comp.name(),
                    axis,
                    idx,
                    dims,
                    &data,
                )));
            }
            next_report = t0.elapsed().as_secs_f64() + cfg.every_s;
        }
    }
    be.synchronize()?;
    let secs = t0.elapsed().as_secs_f64();
    let raw = be.download()?;
    let plan = be.plan().clone();
    let mut res = assemble(&[&plan], &[raw]);
    res.stats = RunStats {
        backend: be.name(),
        cells: total_cells,
        steps: steps_done,
        seconds: secs,
        mcells_per_s: total_cells as f64 * steps_done as f64 / secs.max(1e-9) / 1e6,
        dt: p0.dt,
        stopped_early: stopped,
        energy_steps,
        energy: energy_trace,
    };
    if plan.media.iter().any(|m| m.gain.is_some())
        && let Some(l) = be.gain_levels()?
    {
        res.gain_levels = l.to_vec();
    }
    finalize(&mut res, &plan);
    let _ = sim;
    Ok(res)
}

/// One plane of a field component in lightsim's storage layout
/// (`((i + 1) * ny + j) * nz + k`, one ghost plane on each side along x).
fn plane(
    f: &[lightsim_core::Real],
    n: [usize; 3],
    axis: usize,
    index: usize,
) -> ([usize; 2], Vec<f32>) {
    let (nx, ny, nz) = (n[0], n[1], n[2]);
    let s = |i: usize, j: usize, k: usize| ((i + 1) * ny + j) * nz + k;
    match axis {
        0 => {
            let mut d = Vec::with_capacity(ny * nz);
            for j in 0..ny {
                for k in 0..nz {
                    d.push(f[s(index, j, k)]);
                }
            }
            ([ny, nz], d)
        }
        1 => {
            let mut d = Vec::with_capacity(nx * nz);
            for i in 0..nx {
                for k in 0..nz {
                    d.push(f[s(i, index, k)]);
                }
            }
            ([nx, nz], d)
        }
        _ => {
            let mut d = Vec::with_capacity(nx * ny);
            for i in 0..nx {
                for j in 0..ny {
                    d.push(f[s(i, j, index)]);
                }
            }
            ([nx, ny], d)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_run_matches_lightsim() {
        let sim = crate::setup::tests::wire();
        let mut msgs = vec![];
        let cfg = RunConfig {
            device: Device::Cpu,
            slice: Some(SliceRequest::default_for(&sim)),
            every_s: 0.0,
        };
        let res = run(&sim, &cfg, Arc::new(AtomicBool::new(false)), &mut |m| {
            msgs.push(m)
        })
        .unwrap();
        let reference = lightsim_core::run_cpu(&sim).unwrap();
        assert_eq!(res.stats.steps, reference.stats.steps);
        let a = &res.flux["through"];
        let b = &reference.flux["through"];
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() <= 1e-6 * y.abs().max(1e-30), "{x} vs {y}");
        }
        assert!(msgs.iter().any(|m| matches!(m, Msg::Slice(_))));
        assert!(msgs.iter().any(|m| matches!(m, Msg::Progress { .. })));
    }

    #[test]
    fn cancel_stops() {
        let sim = crate::setup::tests::wire();
        let cancel = Arc::new(AtomicBool::new(true));
        let cfg = RunConfig {
            device: Device::Cpu,
            slice: None,
            every_s: 1.0,
        };
        let res = run(&sim, &cfg, cancel, &mut |_| {}).unwrap();
        assert!(res.stats.stopped_early);
        assert_eq!(res.stats.steps, 1);
    }
}
