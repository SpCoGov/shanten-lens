//! Shanten Lens 3.0 backend core.

extern crate self as shanten_backend;

mod http_trace;
pub mod ipc;
pub mod logging;
pub mod pipeline;
pub mod plugins;
pub mod protocol;
pub mod proxy;
pub mod runtime;
pub mod services;
mod storage;
pub mod upstream;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub mod automation;
pub mod action_provider;
pub mod action_executor;
pub mod decision;
pub mod direct_connection;
pub mod embedded;
pub mod mail;
pub mod recommendations;
