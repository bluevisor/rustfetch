//! Shared functions across operating systems

mod shared;
pub use shared::*;

mod shell;
pub use shell::*;

mod network;
pub use network::*;

mod tailscale;
pub use tailscale::*;
