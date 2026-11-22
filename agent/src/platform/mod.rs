//! Everything that differs per OS: capture, audio, encoders, display lookup and input
//! injection. Each OS module provides the items re-exported below, and shared code uses only
//! these.

#[cfg(windows)]
mod windows;
// `self::` because a bare `windows` would also name the `windows` crate.
#[cfg(windows)]
use self::windows as os;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use self::linux as os;

#[cfg(target_os = "linux")]
pub use os::EisConnection;
#[cfg(windows)]
pub use os::find_display_gpu;
pub use os::{
    Capture, DisplayGpu, ENCODERS, InputInjector, TouchInjector, audio_capture, audio_unavailable,
    output_caps, runs_on, show_cursor, use_physical_pixels,
};

/// An encoder this OS can use, if it is installed.
pub struct EncoderDefinition {
    pub id: &'static str,
    pub label: &'static str,
    /// Matches the element factory names that provide it.
    pub factory_pattern: &'static str,
    /// Whether it encodes on a GPU, i.e. its factory has the Hardware class.
    pub on_gpu: bool,
    /// The encoder bin's description, from the factory name and the bitrate in kbps.
    pub describe: fn(&str, i32) -> String,
}
