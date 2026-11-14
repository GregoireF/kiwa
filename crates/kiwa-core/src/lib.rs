//! kiwa-core: zero-config, polyglot project and task discovery.
//!
//! Design rules (see `docs/explanation/architecture.md`):
//! - Discovery is **static**: it reads manifest files and never executes project code.
//! - Everything is serializable: the CLI, the MCP server and the TUI share one model.
//! - No global state; every entry point takes explicit options.

pub mod about;
pub mod config;
pub mod detect;
pub mod discover;
pub mod fuzzy;
pub mod git;
pub mod model;
pub mod ports;
pub mod resolve;
pub mod setup;
pub mod sources;
pub mod state;
pub mod suggest;
pub mod words;
pub mod workspace;

pub use discover::{DiscoverOptions, discover};
pub use model::{Ecosystem, Project, Task};

/// Version of the machine-readable output contract (`--json`, MCP).
/// Bumped only on breaking changes to the JSON shapes. See `docs/reference/json-and-exit-codes.md`.
pub const SCHEMA: &str = "kiwa/v1";
