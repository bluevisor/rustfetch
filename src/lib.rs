//! Exposes all modules for testing
// Only Linux caches its GPU name, macOS reads it from IOKit on every run
#[cfg(target_os = "linux")]
pub mod cache;
pub mod cli;
pub mod common;
pub mod config;
pub mod platform;
pub mod sysinfo;
