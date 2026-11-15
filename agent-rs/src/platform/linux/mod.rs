//! Linux capture and encoders for Wayland desktops: the screen comes from the ScreenCast portal
//! through PipeWire. Audio and input aren't ported yet.

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::sync::Once;

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, ResponseError, Session};
use ashpd::zbus::block_on;
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::{debug, warn};

use crate::encoders::EncoderDefinition;
use crate::input::touch::{TouchBackend, TouchPoint};
use crate::input::{InputBackend, MouseButton};
use crate::settings::Resolution;

// GNOME sends frames only when the screen changes; pipewiresrc resends the last one this often
// so a static screen keeps streaming.
const KEEPALIVE_MS: u32 = 100;

/// A ScreenCast portal session sharing one monitor, held for as long as the stream runs.
/// Dropping it ends the session and GNOME's sharing indicator.
pub struct Capture {
    session: Session<Screencast>,
    // pipewiresrc connects with a duplicate of this PipeWire remote.
    remote: OwnedFd,
    node_id: u32,
    size: Option<Resolution>,
}

impl Capture {
    /// Asks the portal to share a monitor. The first time, GNOME shows a dialog on the desktop;
    /// a saved restore token skips it afterwards. Blocks until the dialog is answered.
    pub fn open() -> Result<Self, String> {
        block_on(open_session()).map_err(|error| match error {
            ashpd::Error::Response(ResponseError::Cancelled) => {
                "Screen sharing was declined.".to_owned()
            }
            error => format!("Could not start screen sharing: {error}"),
        })
    }

    /// The capture segment of the pipeline; its source is named "capture". GNOME's stream has a
    /// variable frame rate, so videorate caps it at the output caps' rate. The portal's format
    /// never changes; see `attach`.
    pub fn description(&self) -> String {
        format!(
            "pipewiresrc fd={fd} path={node} name=capture keepalive-time={KEEPALIVE_MS} ! \
             videorate drop-only=true ! videoconvert ! videoscale",
            fd = self.remote.as_raw_fd(),
            node = self.node_id,
        )
    }

    /// Takes the element named "capture" once the pipeline is built. A settings change sends a
    /// reconfigure event upstream, and pipewiresrc answers it by reconnecting its stream, after
    /// which GNOME sends no more frames. The elements after it adapt to any change, so it never
    /// needs to renegotiate. The event is dropped on the next element's sink pad, since
    /// arriving on pipewiresrc's pad already flags it for renegotiation.
    pub fn attach(&self, capture: &gst::Element) {
        let Some(next_sink) = capture.static_pad("src").and_then(|src| src.peer()) else {
            return;
        };
        next_sink.add_probe(gst::PadProbeType::EVENT_UPSTREAM, |_, info| {
            match info.event() {
                Some(event) if event.type_() == gst::EventType::Reconfigure => {
                    gst::PadProbeReturn::Drop
                }
                _ => gst::PadProbeReturn::Ok,
            }
        });
    }

    /// The shared monitor's size, as the portal reports it.
    pub fn size(&self) -> Option<Resolution> {
        self.size
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Err(error) = block_on(self.session.close()) {
            debug!("Could not close the screen cast session: {error}");
        }
    }
}

async fn open_session() -> ashpd::Result<Capture> {
    let portal = Screencast::new().await?;
    let session = portal.create_session(Default::default()).await?;
    let saved_token = restore_token_path().and_then(|path| std::fs::read_to_string(path).ok());
    portal
        .select_sources(
            &session,
            SelectSourcesOptions::default()
                .set_sources(SourceType::MONITOR)
                .set_multiple(false)
                .set_cursor_mode(CursorMode::EMBEDDED)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(saved_token.as_deref().map(str::trim)),
        )
        .await?;
    let streams = portal
        .start(&session, None, Default::default())
        .await?
        .response()?;
    if let Some(token) = streams.restore_token() {
        save_restore_token(token);
    }
    let stream = streams
        .streams()
        .first()
        .ok_or(ashpd::Error::Response(ResponseError::Other))?;
    let size = stream
        .size()
        .map(|(width, height)| Resolution { width, height });
    let node_id = stream.pipe_wire_node_id();
    let remote = portal
        .open_pipe_wire_remote(&session, Default::default())
        .await?;
    Ok(Capture {
        session,
        remote,
        node_id,
        size,
    })
}

// The token lets later sessions, also after a restart, share the same monitor without the
// dialog. Each session returns a new one.
fn restore_token_path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(state.join("web-remote-desktop/screencast-restore-token"))
}

fn save_restore_token(token: &str) {
    let Some(path) = restore_token_path() else {
        return;
    };
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, token));
    if let Err(error) = saved {
        warn!(
            "Could not save the screen cast restore token to {}: {error}",
            path.display()
        );
    }
}

pub fn audio_capture() -> String {
    unreachable!("audio_unavailable() keeps sessions video-only")
}

/// Why audio can't stream on this computer, or None if it can.
pub fn audio_unavailable() -> Option<&'static str> {
    Some("not ported yet")
}

/// The portal fixes the cursor mode when the session starts, so the stream always shows the
/// cursor.
pub fn show_cursor(_capture: &gst::Element, _visible: bool) {
    static LOGGED: Once = Once::new();
    LOGGED.call_once(|| debug!("Hiding the stream cursor isn't supported on Linux"));
}

// videoscale scales to the output size and videorate caps the frame rate. Changing these caps
// while playing renegotiates the pipeline as on Windows.
pub fn output_caps(size: Option<Resolution>, framerate: i32) -> String {
    let size = size.map_or(String::new(), |size| {
        format!("width={},height={},", size.width, size.height)
    });
    format!("video/x-raw,format=NV12,{size}framerate={framerate}/1")
}

// Each describes the encoder segment between the capture's NV12 output in system memory and
// h264parse, as on Windows. In priority order: the first available one is the default. The GPU
// encoders aren't offered until the display's GPU can be found, and are untested.
pub static ENCODERS: &[EncoderDefinition] = &[
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
        id: "nvenc",
        label: "NVIDIA NVENC",
        factory_pattern: "^nvh264enc$",
        on_gpu: true,
        describe: |factory, kbps| {
            format!("{factory} name=encoder zerolatency=true bframes=0 gop-size=60 bitrate={kbps}")
        },
    },
];

/// The GPU that drives the captured display.
pub struct DisplayGpu {
    pub description: String,
}

// Not ported yet: VA elements register per render node (vah264enc, varenderD129h264enc, …),
// which would need matching to the display's GPU.
pub fn find_display_gpu() -> Option<DisplayGpu> {
    None
}

pub fn runs_on(_factory: &gst::ElementFactory, _gpu: &DisplayGpu) -> bool {
    false
}

/// Nothing to do: the portal stream and its size are already in physical pixels.
pub fn use_physical_pixels() {}

/// Not ported yet; drops all input.
pub struct InputInjector;

impl InputInjector {
    pub fn new() -> Self {
        Self
    }
}

impl InputBackend for InputInjector {
    fn move_to(&mut self, _x: f64, _y: f64) {}
    fn move_by(&mut self, _dx: i32, _dy: i32) {}
    fn button(&mut self, _button: MouseButton, _down: bool) {}
    fn scroll(&mut self, _dx: i32, _dy: i32) {}
    fn key(&mut self, _code: &str, _down: bool) -> bool {
        false
    }
}

/// Not ported yet; drops all touch.
pub struct TouchInjector;

impl TouchInjector {
    pub fn new() -> Self {
        Self
    }
}

impl TouchBackend for TouchInjector {
    // Not 0×0, which would make to_pixel's clamp range empty.
    fn display_size(&self) -> (i32, i32) {
        (1, 1)
    }

    fn send(&mut self, _frame: &[TouchPoint]) {}
}
