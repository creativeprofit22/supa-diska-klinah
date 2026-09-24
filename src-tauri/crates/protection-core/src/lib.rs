//! Platform-neutral protection primitives.
//!
//! This crate knows nothing about Windows, Tauri or the network. It owns the
//! signed rule-pack format, the matching engine, the heuristic catalog, the
//! typed evidence model and the network opt-in policy (ADR 0003).

mod engine;
mod evidence;
mod heuristics;
mod pack;
mod policy;

pub use engine::*;
pub use evidence::*;
pub use heuristics::*;
pub use pack::*;
pub use policy::*;
