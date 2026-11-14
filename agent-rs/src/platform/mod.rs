//! Everything that differs per OS: capture, audio, encoders, display lookup and input
//! injection. Shared code calls the same names on every OS.

#[cfg(windows)]
mod windows;
