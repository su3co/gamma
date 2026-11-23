//! The solvers a simulation can name (`solver = "..."` in `gamma.toml`).
//!
//! gamma 0.1 runs lightsim's FDTD solver. lightsim is adding a
//! finite-element solver (heat, electrostatics, modes), eigenmode expansion,
//! circuit simulation and a fixed-angle oblique-incidence mode; each will be
//! a solver here with its own set-up summary, runner and result kinds, so the
//! interface asks the solver what to show instead of assuming FDTD.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct SolverInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub engine: &'static str,
    pub available: bool,
    /// Result kinds the results view can show for this solver.
    pub results: &'static [&'static str],
}

pub const SOLVERS: &[SolverInfo] = &[
    SolverInfo {
        id: "fdtd",
        name: "FDTD",
        engine: "lightsim",
        available: true,
        results: &[
            "sparams",
            "spectrum",
            "field_map",
            "time_series",
            "far_field",
            "decay",
        ],
    },
    SolverInfo {
        id: "fem",
        name: "Finite elements (heat, electrostatics, modes)",
        engine: "lightsim",
        available: false,
        results: &[],
    },
    SolverInfo {
        id: "eme",
        name: "Eigenmode expansion",
        engine: "lightsim",
        available: false,
        results: &[],
    },
    SolverInfo {
        id: "circuit",
        name: "Circuit (S-parameter networks)",
        engine: "lightsim",
        available: false,
        results: &[],
    },
    SolverInfo {
        id: "quantum",
        name: "Quantum circuits",
        engine: "lightsim-quantum",
        available: false,
        results: &[],
    },
];

pub fn get(id: &str) -> Option<&'static SolverInfo> {
    SOLVERS.iter().find(|s| s.id == id)
}

/// Check that a simulation's solver can run in this build.
pub fn require(id: &str) -> anyhow::Result<&'static SolverInfo> {
    match get(id) {
        Some(s) if s.available => Ok(s),
        Some(s) => anyhow::bail!(
            "the {} solver ({}) is not available in gamma {} yet",
            s.name,
            s.engine,
            gamma_core::VERSION
        ),
        None => anyhow::bail!(
            "unknown solver '{id}'; known: {}",
            SOLVERS.iter().map(|s| s.id).collect::<Vec<_>>().join(", ")
        ),
    }
}
