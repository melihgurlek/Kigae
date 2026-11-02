//! Kigae's apply engine. No Qt here, so it builds and tests anywhere (SPEC.md §1).

pub mod capture;
pub mod card;
pub mod config;
pub mod daynight;
pub mod discover;
pub mod engine;
pub mod install;
pub mod journal;
pub mod kconfig;
pub mod manifest;
pub mod sys;
pub mod vscode;

pub use manifest::{Manifest, ManifestError};
