//! One viewer's stream: a capture → encode → webrtcbin pipeline that answers the viewer's SDP
//! offer and runs until it is stopped or the connection ends.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use gstreamer as gst;
use gstreamer::glib;
use gstreamer::prelude::*;
use gstreamer_sdp as gst_sdp;
use gstreamer_webrtc::{
    WebRTCICEGatheringState, WebRTCPeerConnectionState, WebRTCSDPType, WebRTCSessionDescription,
};
use tracing::{debug, error, info, warn};

use crate::encoders::EncoderCatalog;
use crate::platform;
use crate::sdp::{H264Codec, InvalidOffer};
use crate::settings::{
    DEFAULT_BITRATE_KBPS, DEFAULT_FRAMERATE, Resolution, SizeLimits, StreamSettings,
};

const SETUP_TIMEOUT: Duration = Duration::from_secs(10);
// How long the viewer may take to connect after setup before the session ends.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
// How long a "disconnected" connection may take to recover before the session ends.
const DISCONNECT_GRACE: Duration = Duration::from_secs(10);

/// Why a session couldn't start.
#[derive(Debug)]
pub enum StartError {
    /// The viewer's offer can't be answered.
    InvalidOffer(String),
    Failed(String),
}

impl From<InvalidOffer> for StartError {
    fn from(error: InvalidOffer) -> Self {
        Self::InvalidOffer(error.0)
    }
}

fn failed(message: impl ToString) -> StartError {
    StartError::Failed(message.to_string())
}

pub struct StreamSession {
    id: String,
    pipeline: gst::Pipeline,
    webrtc: gst::Element,
    capture_src: gst::Pad,
    output_src: gst::Pad,
    settings: Mutex<StreamSettings>,
    end_reason: OnceLock<String>,
    on_ended: Box<dyn Fn(&str) + Send + Sync>,
    stopped: AtomicBool,
    // Dropping the sender stops the connection monitor.
    stop_monitor: Mutex<Option<mpsc::Sender<()>>>,
}

impl StreamSession {
    /// Starts streaming and returns the session and its SDP answer. Blocks for up to a few
    /// seconds. `on_ended` is called once, from a background thread, when the connection fails
    /// or the pipeline errors.
    pub fn start(
        offer_sdp: &str,
        codec: &H264Codec,
        encoders: &EncoderCatalog,
        on_ended: impl Fn(&str) + Send + Sync + 'static,
    ) -> Result<(Arc<Self>, String), StartError> {
        let encoder = encoders.default();
        let size = initial_size(encoders.limits(encoder));
        let pipeline = gst::parse::launch(&pipeline_description(codec, size))
            .map_err(failed)?
            .downcast::<gst::Pipeline>()
            .map_err(|_| failed("The pipeline description is not a pipeline."))?;
        let webrtc = by_name(&pipeline, "webrtc")?;
        let output_src = static_pad(&by_name(&pipeline, "output")?, "src")?;
        let parse_sink = static_pad(&by_name(&pipeline, "parse")?, "sink")?;

        // The encoder bin goes between the capsfilter and h264parse, which the pipeline
        // description leaves unlinked.
        let bin = gst::parse::bin_from_description(
            &encoders.describe(encoder, DEFAULT_BITRATE_KBPS),
            true,
        )
        .map_err(failed)?;
        pipeline.add(&bin).map_err(failed)?;
        let linked = output_src.link(&static_pad(&bin, "sink")?).is_ok()
            && static_pad(&bin, "src")?.link(&parse_sink).is_ok();
        if !linked {
            return Err(failed(format!("Could not link the {encoder} encoder.")));
        }

        let session = Arc::new(Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            capture_src: static_pad(&by_name(&pipeline, "capture")?, "src")?,
            output_src,
            pipeline,
            webrtc,
            settings: Mutex::new(StreamSettings {
                width: 0,
                height: 0,
                framerate: DEFAULT_FRAMERATE,
                bitrate_kbps: DEFAULT_BITRATE_KBPS,
                encoder: encoder.into(),
            }),
            end_reason: OnceLock::new(),
            on_ended: Box::new(on_ended),
            stopped: AtomicBool::new(false),
            stop_monitor: Mutex::new(None),
        });
        match session.begin(offer_sdp) {
            Ok(answer) => Ok((session, answer)),
            Err(error) => {
                session.stop();
                Err(error)
            }
        }
    }

    fn begin(self: &Arc<Self>, offer_sdp: &str) -> Result<String, StartError> {
        self.watch_bus();
        // A failure posts an error on the bus, which the waits in answer() report.
        let _ = self.pipeline.set_state(gst::State::Playing);
        let answer = self.answer(offer_sdp)?;

        // answer() waits for the first frame, so the output caps are known by now.
        let size = caps_size(&self.output_src);
        let mut settings = self.settings.lock().unwrap();
        settings.width = size.width;
        settings.height = size.height;
        drop(settings);

        self.monitor_connection();
        Ok(answer)
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// The stream's current settings.
    pub fn settings(&self) -> StreamSettings {
        self.settings.lock().unwrap().clone()
    }

    /// The captured desktop's resolution.
    pub fn source_resolution(&self) -> Resolution {
        caps_size(&self.capture_src)
    }

    /// Why the session ended, or None while it runs.
    pub fn end_reason(&self) -> Option<&str> {
        self.end_reason.get().map(String::as_str)
    }

    /// Stops streaming and releases the capture. Does nothing the second time.
    pub fn stop(&self) {
        if self.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        self.stop_monitor.lock().unwrap().take();
        let _ = self.pipeline.set_state(gst::State::Null);
    }

    fn answer(&self, offer_sdp: &str) -> Result<String, StartError> {
        // Until the first frame arrives our video pad has no caps, and webrtcbin can't match it
        // to the offer; it would answer with an unrelated, inactive transceiver.
        let video_pad = static_pad(&self.webrtc, "sink_0")?;
        self.wait_until(
            || video_pad.current_caps().is_some(),
            "No video from the capture pipeline.",
        )?;

        let sdp = gst_sdp::SDPMessage::parse_buffer(offer_sdp.as_bytes())
            .map_err(|_| StartError::InvalidOffer("The offer is not valid SDP.".into()))?;
        let offer = WebRTCSessionDescription::new(WebRTCSDPType::Offer, sdp);
        self.emit_and_wait("set-remote-description", &offer)?;
        let reply = self.emit_and_wait("create-answer", &None::<gst::Structure>)?;
        let answer = reply
            .and_then(|reply| reply.get::<WebRTCSessionDescription>("answer").ok())
            .ok_or_else(|| failed("webrtcbin create-answer replied without an answer."))?;
        self.emit_and_wait("set-local-description", &answer)?;

        // Non-trickle ICE: wait until every local candidate is in the local description.
        self.wait_until(
            || {
                self.webrtc
                    .property::<WebRTCICEGatheringState>("ice-gathering-state")
                    == WebRTCICEGatheringState::Complete
            },
            "ICE gathering did not complete.",
        )?;
        let local = self
            .webrtc
            .property::<Option<WebRTCSessionDescription>>("local-description")
            .ok_or_else(|| failed("webrtcbin has no local description."))?;
        local.sdp().as_text().map_err(failed)
    }

    // webrtcbin's action signals take a GstPromise as their last argument and resolve it when
    // done. Returns the promise's reply.
    fn emit_and_wait(
        &self,
        signal: &str,
        argument: &dyn ToValue,
    ) -> Result<Option<gst::Structure>, StartError> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let promise = gst::Promise::with_change_func(move |reply| {
            let _ = sender.send(reply.map(|reply| reply.map(ToOwned::to_owned)));
        });
        self.webrtc
            .emit_by_name::<()>(signal, &[argument, &promise]);
        let Ok(Ok(reply)) = receiver.recv_timeout(SETUP_TIMEOUT) else {
            // Interrupting releases a reply webrtcbin may still send.
            promise.interrupt();
            return Err(failed(format!("webrtcbin {signal} did not reply.")));
        };

        // webrtcbin reports failures as an "error" field in the reply. Every signal here
        // negotiates the viewer's offer, so a failure means the offer can't be answered.
        if let Some(reply) = &reply
            && reply.has_field("error")
        {
            let message = reply.get::<glib::Error>("error").map_or_else(
                |_| format!("{signal} failed"),
                |error| error.message().into(),
            );
            return Err(StartError::InvalidOffer(format!(
                "The offer was rejected: {message}"
            )));
        }
        Ok(reply)
    }

    fn wait_until(
        &self,
        condition: impl Fn() -> bool,
        timeout_message: &str,
    ) -> Result<(), StartError> {
        let deadline = Instant::now() + SETUP_TIMEOUT;
        while !condition() {
            // Fail with the pipeline's own error instead of waiting out the timeout.
            if let Some(reason) = self.end_reason() {
                return Err(failed(reason));
            }
            if Instant::now() > deadline {
                return Err(failed(timeout_message));
            }
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }

    fn watch_bus(self: &Arc<Self>) {
        let bus = self.pipeline.bus().expect("a pipeline has a bus");
        let session = Arc::downgrade(self);
        thread::Builder::new()
            .name(format!("bus {}", self.id))
            .spawn(move || {
                loop {
                    let message = bus.timed_pop_filtered(
                        gst::ClockTime::from_mseconds(200),
                        &[gst::MessageType::Error, gst::MessageType::Warning],
                    );
                    let Some(session) = session.upgrade() else {
                        return;
                    };
                    if session.stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(message) = message
                        && !session.handle_bus_message(&message)
                    {
                        return;
                    }
                }
            })
            .expect("could not start the bus thread");
    }

    // Returns false once the session has ended.
    fn handle_bus_message(&self, message: &gst::Message) -> bool {
        let source = message
            .src()
            .map_or_else(String::new, |source| source.name().to_string());
        // The message structure holds both the error text and the debug details.
        let details = message
            .structure()
            .map_or_else(String::new, |structure| structure.to_string());
        match message.view() {
            // SCTP carries the input channel; its association fails when the viewer closes the
            // connection. The video keeps going, and the connection monitor ends the session.
            gst::MessageView::Error(_) if source.starts_with("sctp") => {
                info!(
                    "Session {}: input channel failed: {source} association ended",
                    self.id
                );
                true
            }
            gst::MessageView::Warning(warning) => {
                // A frame rate change makes d3d11convert fail to renegotiate with the old input
                // caps until the capture source renegotiates upstream. It's expected and
                // recovers at once.
                let debug = warning.debug().unwrap_or_default();
                if debug.contains("gst_base_transform_reconfigure")
                    && debug.contains("not negotiated")
                {
                    debug!("Session {} renegotiating after a settings change", self.id);
                } else {
                    warn!(
                        "Session {} pipeline warning from {source}: {details}",
                        self.id
                    );
                }
                true
            }
            _ => {
                error!(
                    "Session {} pipeline error from {source}: {details}",
                    self.id
                );
                self.end(format!("Pipeline error from {source}: {details}"));
                false
            }
        }
    }

    fn monitor_connection(self: &Arc<Self>) {
        let (stop_sender, stop) = mpsc::channel::<()>();
        *self.stop_monitor.lock().unwrap() = Some(stop_sender);
        let session = Arc::downgrade(self);
        let mut connect_deadline = Some(Instant::now() + CONNECT_TIMEOUT);
        let mut disconnected_since = None;
        thread::Builder::new()
            .name(format!("connection {}", self.id))
            .spawn(move || {
                // Checks every second until stop() drops the sender.
                while let Err(RecvTimeoutError::Timeout) = stop.recv_timeout(Duration::from_secs(1))
                {
                    let Some(session) = session.upgrade() else {
                        return;
                    };
                    if !session.check_connection(&mut connect_deadline, &mut disconnected_since) {
                        return;
                    }
                }
            })
            .expect("could not start the connection monitor");
    }

    // Returns false once the session has ended.
    fn check_connection(
        &self,
        connect_deadline: &mut Option<Instant>,
        disconnected_since: &mut Option<Instant>,
    ) -> bool {
        use WebRTCPeerConnectionState::{Closed, Connected, Disconnected, Failed};

        let state = self
            .webrtc
            .property::<WebRTCPeerConnectionState>("connection-state");
        if state == Connected {
            *connect_deadline = None;
        }
        if state == Disconnected {
            disconnected_since.get_or_insert_with(Instant::now);
        } else {
            *disconnected_since = None;
        }

        let never_connected = connect_deadline.is_some_and(|deadline| Instant::now() > deadline);
        let lost_too_long =
            disconnected_since.is_some_and(|since| since.elapsed() > DISCONNECT_GRACE);
        if !(matches!(state, Failed | Closed) || lost_too_long || never_connected) {
            return true;
        }
        let reason = if never_connected {
            format!("Viewer did not connect (connection {state:?})")
        } else {
            format!("Connection {state:?}")
        };
        info!("Session {} ended: {reason}", self.id);
        self.end(reason);
        false
    }

    // The session can end during setup, before ViewerSessions makes it current. Recording the
    // reason first lets setup and ViewerSessions see it either way.
    fn end(&self, reason: String) {
        if self.end_reason.set(reason).is_err() {
            return;
        }
        if !self.stopped.load(Ordering::SeqCst) {
            (self.on_ended)(&self.id);
        }
    }
}

impl Drop for StreamSession {
    fn drop(&mut self) {
        self.stop();
    }
}

// webrtcbin matches our stream to the offer by intersecting RTP caps exactly, so the payload
// type and profile-level-id must be the browser's. Desktop resolutions need a higher H.264 level
// than browsers offer; they accept it (level-asymmetry-allowed=1), so capssetter just relabels
// the caps. AMF labels its constrained output plain "baseline", so accept both. The encoder bin
// goes between the two chains; see StreamSession::start.
fn pipeline_description(codec: &H264Codec, size: Option<Resolution>) -> String {
    format!(
        "{capture} ! capsfilter name=output caps={caps}  \
         h264parse name=parse ! video/x-h264,profile=(string){{constrained-baseline,baseline}} ! \
         rtph264pay pt={payload_type} config-interval=-1 aggregate-mode=zero-latency ! \
         capssetter caps=\"application/x-rtp,profile-level-id=(string){profile_level_id}\" ! \
         webrtcbin name=webrtc bundle-policy=max-bundle",
        capture = platform::CAPTURE,
        caps = platform::output_caps(size, DEFAULT_FRAMERATE),
        payload_type = codec.payload_type,
        profile_level_id = codec.profile_level_id,
    )
}

// Streams start at native resolution (None), unless the display is larger than the encoder
// accepts; then they start scaled down to fit, e.g. NVENC's 4096 px on a 5K display.
fn initial_size(limits: SizeLimits) -> Option<Resolution> {
    let display = platform::primary_display_size()?;
    (!limits.fits(display)).then(|| limits.shrink_to_fit(display))
}

/// The size in a pad's caps, or 0×0 before the first frame.
fn caps_size(pad: &gst::Pad) -> Resolution {
    let caps = pad.current_caps();
    let structure = caps.as_ref().and_then(|caps| caps.structure(0));
    let field = |name| {
        structure
            .and_then(|structure| structure.get::<i32>(name).ok())
            .unwrap_or(0)
    };
    Resolution {
        width: field("width"),
        height: field("height"),
    }
}

fn by_name(bin: &impl IsA<gst::Bin>, name: &str) -> Result<gst::Element, StartError> {
    bin.by_name(name)
        .ok_or_else(|| failed(format!("Element {name} not found.")))
}

fn static_pad(element: &impl IsA<gst::Element>, name: &str) -> Result<gst::Pad, StartError> {
    element
        .static_pad(name)
        .ok_or_else(|| failed(format!("Pad {name} not found.")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_the_same_pipeline_as_the_csharp_agent() {
        let codec = H264Codec {
            payload_type: 106,
            profile_level_id: "42e01f".into(),
        };
        let size = Resolution {
            width: 4096,
            height: 2304,
        };

        assert_eq!(
            pipeline_description(&codec, Some(size)),
            "d3d11screencapturesrc name=capture show-cursor=true ! d3d11convert ! \
             capsfilter name=output caps=video/x-raw(memory:D3D11Memory),format=NV12,width=4096,height=2304,framerate=30/1  \
             h264parse name=parse ! video/x-h264,profile=(string){constrained-baseline,baseline} ! \
             rtph264pay pt=106 config-interval=-1 aggregate-mode=zero-latency ! \
             capssetter caps=\"application/x-rtp,profile-level-id=(string)42e01f\" ! \
             webrtcbin name=webrtc bundle-policy=max-bundle"
        );
        assert!(
            pipeline_description(&codec, None)
                .contains("caps=video/x-raw(memory:D3D11Memory),format=NV12,framerate=30/1  ")
        );
    }
}
