//! sim-api: a thin, stateless HTTP shell over sim-core
//! (specs/sim-api-plan.md). It's the only crate with async code, and every
//! simulation still runs synchronously inside it.

mod app;
mod config;
mod error;

pub use app::app;
pub use config::{Config, ConfigError, DEFAULT_PORT};
