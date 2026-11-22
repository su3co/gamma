//! # gamma-hub
//!
//! One hub per machine and user owns everything that changes: the projects
//! that are open, their event logs, file watchers, git history, simulation
//! jobs (local and remote) and agent processes. Windows, browser tabs, the
//! MCP server and the CLI are its clients, over HTTP and a WebSocket on the
//! loopback interface, authenticated by a token written to the runtime
//! directory.

pub mod agent;
pub mod api;
pub mod config;
pub mod discovery;
pub mod events;
pub mod git;
pub mod jobs;
pub mod projects;
pub mod remote;
pub mod state;
pub mod views;
pub mod watch;

pub use discovery::{HubInfo, connect_or_spawn, read_hub_info};
pub use state::Hub;
