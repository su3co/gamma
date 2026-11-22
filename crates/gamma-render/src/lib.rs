//! # gamma-render
//!
//! Draws gamma's scenes without a window, to PNG: the layout, the extruded
//! stack, the simulation set-up, field maps and line plots. Agents use these
//! images to look at what they made (`view_render`), the CLI writes them
//! (`gamma view`), and the hub uses them for thumbnails. The interactive
//! window draws the same scenes with WebGL2.

pub mod colormap;
pub mod plot;
pub mod scene;
pub mod text;

pub use scene::{Theme, render_field, render_layout, render_stack3d};

/// A finished image.
pub struct Png(pub Vec<u8>);
