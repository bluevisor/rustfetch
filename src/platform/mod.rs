//! Platform specific information functions
//! This module compiles platform-specific implementation at compile time based on the target OS

// Linux and Haiku executables are both ELF
#[cfg(any(target_os = "linux", target_os = "haiku"))]
mod elf;
#[cfg(any(target_os = "linux", target_os = "haiku"))]
pub use elf::*;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(target_os = "haiku")]
mod haiku;
#[cfg(target_os = "haiku")]
pub use haiku::*;
