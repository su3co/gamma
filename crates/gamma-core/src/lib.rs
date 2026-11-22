//! # gamma-core
//!
//! The parts of gamma that do not depend on a solver or a window:
//!
//! * [`project`] — the project folder, `gamma.toml` and its discovery,
//! * [`stack`] — layer stack and materials (`stack.toml`),
//! * [`gds`] — a GDSII reader that flattens a cell into polygons per layer,
//! * [`geom`] — polygons, triangulation and extrusion,
//! * [`layout`] — the layout model the viewers and tools query (cells, layers,
//!   ports, picking, measuring) and the layout diff of two checkpoints,
//! * [`checks`] — design-rule and consistency checks,
//! * [`refs`] — stable text references to objects (`port:o1`, `layer:1/0`),
//! * [`fsutil`] — crash-safe file writes.

pub mod checks;
pub mod fsutil;
pub mod gds;
pub mod geom;
pub mod layout;
pub mod netlist;
pub mod project;
pub mod refs;
pub mod stack;

pub use layout::Layout;
pub use project::{Manifest, Project};
pub use stack::Stack;

/// gamma's version, as built.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
