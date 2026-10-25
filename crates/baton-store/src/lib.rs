//! Experimental local metadata engine. No process or filesystem writer authority.

#[cfg(feature = "experimental")]
mod engine;
#[cfg(feature = "experimental")]
pub use engine::*;
