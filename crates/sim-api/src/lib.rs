//! sim-api: a thin, stateless HTTP shell over sim-core
//! (specs/sim-api-plan.md). It's the only crate with async code, and every
//! simulation still runs synchronously inside it.

mod app;
mod config;
mod dto;
pub mod encode;
mod error;
mod routes;

pub use app::{MAX_BODY_BYTES, app};
pub use config::{Config, ConfigError, DEFAULT_PORT};
