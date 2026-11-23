//! # gamma-sim
//!
//! gamma's integration of the lightsim FDTD solver:
//!
//! * [`setup`] — what a simulation file describes, the grid lightsim builds
//!   for it, permittivity slices from lightsim's own rasteriser, estimates,
//! * [`devices`] — CPU and CUDA devices and their measured throughput,
//! * [`runner`] — runs with live progress, energy decay and field slices,
//! * [`stream`] — the newline-delimited JSON a worker writes while it runs,
//! * [`results`] — reduction of lightsim results to spectra, S-parameters
//!   and field maps,
//! * [`checks`] — consistency checks of a simulation against the layout.

pub mod checks;
pub mod devices;
pub mod export;
pub mod results;
pub mod runner;
pub mod setup;
pub mod solvers;
pub mod stream;

pub use lightsim_core;
pub use lightsim_core::Simulation;

/// Version of lightsim gamma is built with (the pinned git revision).
pub const LIGHTSIM_REV: &str = "849fd1a1a86ad164ac8c783f82d099eba45f03e1";
