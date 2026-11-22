//! `gamma worker`: runs one simulation and writes the stream protocol
//! (newline-delimited JSON) on standard output. The hub starts it locally
//! or over SSH on a remote host.

use anyhow::{Context, Result};
use gamma_sim::devices::Device;
use gamma_sim::runner::{RunConfig, SliceRequest, run};
use gamma_sim::stream::Msg;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub fn hello() -> Msg {
    Msg::Hello {
        version: gamma_core::VERSION.into(),
        lightsim: gamma_sim::LIGHTSIM_REV.into(),
        host_arch: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        devices: gamma_sim::devices::local_devices(),
        precision: gamma_sim::devices::precision().into(),
    }
}

fn emit(m: &Msg) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", m.line());
    let _ = out.flush();
}

pub fn parse_slice(s: &str, sim: &gamma_sim::Simulation) -> Result<Option<SliceRequest>> {
    if s == "none" {
        return Ok(None);
    }
    if s == "auto" {
        return Ok(Some(SliceRequest::default_for(sim)));
    }
    // COMP,AXIS,COORD  e.g. Ez,2,1.1e-7
    let f: Vec<&str> = s.split(',').collect();
    if f.len() != 3 {
        anyhow::bail!("--slice is auto, none or COMPONENT,AXIS,COORDINATE (e.g. Ey,2,1.1e-7)");
    }
    let comp = lightsim_component(f[0])?;
    let axis: usize = f[1].parse().context("slice axis")?;
    let coord: f64 = f[2].parse().context("slice coordinate")?;
    Ok(Some(SliceRequest { comp, axis, coord }))
}

fn lightsim_component(s: &str) -> Result<gamma_sim::lightsim_core::grid::Component> {
    gamma_sim::lightsim_core::grid::Component::parse(s).with_context(|| format!("unknown component {s}"))
}

pub fn run_worker(spec: &Path, device: &str, out: Option<&Path>, slice: &str, every: f64) -> Result<()> {
    emit(&hello());
    let res = (|| -> Result<()> {
        let sim = gamma_sim::setup::load(spec)?;
        let dev = Device::parse(device)?;
        let cfg = RunConfig { device: dev, slice: parse_slice(slice, &sim)?, every_s: every };
        let cancel = Arc::new(AtomicBool::new(false));
        // SIGTERM/SIGINT end the run with the results so far
        let c2 = cancel.clone();
        let _ = ctrlc_like(move || c2.store(true, Ordering::Relaxed));
        let results = run(&sim, &cfg, cancel, &mut |m| emit(&m))?;
        let path = out.map(|p| p.to_path_buf()).unwrap_or_else(|| spec.with_file_name("results.json"));
        gamma_core::fsutil::atomic_write(&path, results.to_json().as_bytes())?;
        let s = &results.stats;
        emit(&Msg::Done { steps: s.steps, seconds: s.seconds, mcells_per_s: s.mcells_per_s, stopped_early: s.stopped_early, results: path.to_string_lossy().into(), backend: s.backend.clone() });
        Ok(())
    })();
    if let Err(e) = &res {
        emit(&Msg::Error { message: format!("{e:#}") });
    }
    res
}

/// Install a handler for SIGINT and SIGTERM (Unix) without extra crates.
fn ctrlc_like(f: impl Fn() + Send + 'static) -> Result<()> {
    #[cfg(unix)]
    {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        std::thread::spawn(move || {
            rt.block_on(async {
                use tokio::signal::unix::{SignalKind, signal};
                let mut a = signal(SignalKind::terminate()).unwrap();
                let mut b = signal(SignalKind::interrupt()).unwrap();
                tokio::select! { _ = a.recv() => {}, _ = b.recv() => {} }
                f();
            })
        });
    }
    #[cfg(not(unix))]
    {
        let _ = f;
    }
    Ok(())
}
