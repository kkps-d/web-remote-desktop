//! Linux capture, encoders and input for Wayland desktops: the screen comes from the ScreenCast
//! portal through PipeWire, the audio from PipeWire, and input goes in through uinput.

mod capture;
mod eis;
mod gpu;
mod input;
mod key_codes;
mod touch;
mod wayland_gpu;

pub use capture::Capture;
pub use eis::EisConnection;
pub use gpu::{DisplayGpu, runs_on};
pub use input::InputInjector;
pub use touch::TouchInjector;

use std::os::fd::OwnedFd;
use std::path::PathBuf;
use std::sync::Once;

use gstreamer as gst;
use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType, connect, socket_with};
use tracing::debug;

use super::EncoderDefinition;
use crate::stream_settings::Resolution;

/// Captures what the default output plays (its monitor), for the audio branch. The stream
/// follows the default output when it changes, and captures before the output's volume. Without
/// provide-clock=false the source would be the pipeline clock, so an audio stall would freeze the
/// video too. Unconstrained, pipewiresrc picks mono.
pub fn audio_capture() -> String {
    "pipewiresrc stream-properties=\"props,stream.capture.sink=true\" provide-clock=false ! \
     audio/x-raw,rate=48000,channels=2"
        .into()
}

/// Why audio can't stream on this computer, or None if it can.
pub fn audio_unavailable() -> Option<&'static str> {
    if gst::ElementFactory::find("pipewiresrc").is_none() {
        return Some("GStreamer's PipeWire plugin (gstreamer1.0-pipewire) isn't installed");
    }
    if !pipewire_socket().is_some_and(|socket| socket.exists()) {
        return Some("PipeWire isn't running in this session");
    }
    None
}

// Where PipeWire clients connect, following libpipewire's own lookup.
fn pipewire_socket() -> Option<PathBuf> {
    let remote = std::env::var_os("PIPEWIRE_REMOTE").unwrap_or_else(|| "pipewire-0".into());
    if PathBuf::from(&remote).is_absolute() {
        return Some(remote.into());
    }
    let runtime =
        std::env::var_os("PIPEWIRE_RUNTIME_DIR").or_else(|| std::env::var_os("XDG_RUNTIME_DIR"))?;
    Some(PathBuf::from(runtime).join(remote))
}

/// Open an independent connection so one GStreamer source's errors stay local to it.
pub fn open_pipewire_socket() -> Result<OwnedFd, String> {
    let path = pipewire_socket().ok_or("No PipeWire runtime directory")?;
    let open = || {
        let address = SocketAddrUnix::new(&path)?;
        let fd = socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            None,
        )?;
        connect(&fd, &address)?;
        Ok::<_, rustix::io::Errno>(fd)
    };
    open().map_err(|error| {
        format!(
            "Could not connect to PipeWire at {}: {error}",
            path.display()
        )
    })
}

/// The portal fixes the cursor mode when the session starts, so the stream always shows the
/// cursor.
pub fn show_cursor(_capture: &gst::Element, _visible: bool) {
    static LOGGED: Once = Once::new();
    LOGGED.call_once(|| debug!("Hiding the stream cursor isn't supported on Linux"));
}

// videoscale scales to the output size. Changing these caps while playing renegotiates the
// pipeline as on Windows. The frame rate is set in the capture segment instead; see
// `Capture::description`.
pub fn output_caps(size: Option<Resolution>, _framerate: i32) -> String {
    let size = size.map_or(String::new(), |size| {
        format!(",width={},height={}", size.width, size.height)
    });
    format!("video/x-raw,format=NV12{size}")
}

// These encode the capture's NV12 output in system memory. The first available one is the default.
pub static ENCODERS: &[EncoderDefinition] = &[
    EncoderDefinition {
        id: "nvenc",
        label: "NVIDIA NVENC",
        factory_pattern: r"^nvh264(device\d+)?enc$",
        on_gpu: true,
        describe: |factory, kbps| {
            format!("{factory} name=encoder zerolatency=true bframes=0 gop-size=60 bitrate={kbps}")
        },
    },
    EncoderDefinition {
        id: "va",
        label: "VA-API",
        factory_pattern: r"^va(renderD\d+)?h264(lp)?enc$",
        on_gpu: true,
        describe: |factory, kbps| {
            format!(
                "{factory} name=encoder rate-control=cbr b-frames=0 key-int-max=60 bitrate={kbps}"
            )
        },
    },
    EncoderDefinition {
        id: "x264",
        label: "x264 (CPU)",
        factory_pattern: "^x264enc$",
        on_gpu: false,
        describe: |factory, kbps| {
            format!(
                "{factory} name=encoder tune=zerolatency speed-preset=ultrafast key-int-max=60 bitrate={kbps}"
            )
        },
    },
];

/// Nothing to do: the portal stream and its size are already in physical pixels.
pub fn use_physical_pixels() {}
