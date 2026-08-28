//! Shanten Lens 3.0 backend core.

extern crate self as shanten_backend;

pub mod ipc;
pub mod logging;
pub mod pipeline;
pub mod protocol;
pub mod proxy;
pub mod runtime;
pub mod services;
pub mod upstream;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub mod automation;
#[path = "main.rs"]
pub mod embedded;
pub mod mail;
pub mod recommendations;
