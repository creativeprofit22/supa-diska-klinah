//! Platform-neutral protection primitives.
//!
//! This crate knows nothing about Windows, Tauri or the network. It owns the
//! signed rule-pack format, the matching engine, the heuristic catalog, the
//! typed evidence model, the network opt-in policy (ADR 0003) and the signed
//! app-update manifest.

mod engine;
mod evidence;
mod heuristics;
mod pack;
mod policy;
mod update;

pub use engine::*;
pub use evidence::*;
pub use heuristics::*;
pub use pack::*;
pub use policy::*;
pub use update::*;
