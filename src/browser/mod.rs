//! A local, read-only web browser for vault records. See
//! `docs/superpowers/specs/2026-11-06-fhir-browser-design.md`.

mod assets;
mod guard;
pub mod model;
mod routes;
mod vaults;

pub use routes::router;
pub use vaults::{VaultEntry, Vaults};
