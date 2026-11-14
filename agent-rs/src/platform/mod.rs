//! Everything that differs per OS: capture, audio, encoders, display lookup and input
//! injection. Shared code calls the same names on every OS.

#[cfg(windows)]
mod windows;
// `self::` because a bare `windows` would also name the `windows` crate.
#[cfg(windows)]
pub use self::windows::*;
