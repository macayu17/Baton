//! Opt-in managed Codex source integration. No model-state or transfer guarantees.
#[cfg(feature = "experimental")]
mod client;
#[cfg(feature = "experimental")]
pub use client::{Runtime, Source};
#[cfg(feature = "experimental")]
mod wire;
