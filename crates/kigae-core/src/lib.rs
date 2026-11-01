//! Kigae's apply engine. No Qt here, so it builds and tests anywhere (SPEC.md §1).

pub mod manifest;

pub use manifest::{Manifest, ManifestError};
