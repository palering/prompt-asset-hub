//! Product-level use cases and infrastructure ports for Prompt Asset Hub.
//!
//! UI shells and infrastructure adapters depend on this crate. This crate does
//! not depend on Tauri, `SQLx`, Axum, or a concrete object store.

mod dto;
mod error;
mod ports;
mod service;

pub use dto::*;
pub use error::AppError;
pub use ports::*;
pub use service::{AppService, Clock, SystemClock};
