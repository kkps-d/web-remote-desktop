//! One viewer's stream: a capture → encode → webrtcbin pipeline that answers the viewer's SDP
//! offer and runs until it is stopped or the connection ends.

use std::str::FromStr;
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

use crate::encode_timing::{EncodeTiming, EncodingStats};
use crate::encoders::EncoderCatalog;
use crate::input::InputChannel;
use crate::platform;
use crate::sdp::{H264Codec, InvalidOffer};
use crate::settings::{
    DEFAULT_BITRATE_KBPS, DEFAULT_FRAMERATE, InvalidSettings, Resolution, SizeLimits,
    StreamSettings, StreamSettingsUpdate, plan_update,
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

impl From<String> for StartError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

fn failed(message: impl ToString) -> StartError {
    StartError::Failed(message.to_string())
}

/// Why a settings change wasn't applied.
#[derive(Debug)]
pub enum UpdateError {
    InvalidSettings(String),
    /// The encoder failed; a failed switch also ends the session.
    Failed(String),
    /// The session stopped before the change could start.
    Stopped,
}

impl From<InvalidSettings> for UpdateError {
    fn from(error: InvalidSettings) -> Self {
        Self::InvalidSettings(error.0)
    }
}

impl From<String> for UpdateError {
    fn from(message: String) -> Self {
        Self::Failed(message)
    }
}

/// An encoder bin and the parts of it the session uses.
struct EncoderBin {
    bin: gst::Bin,
    sink: gst::Pad,
    src: gst::Pad,
    /// The element named "encoder", which takes bitrate changes.
    encoder: gst::Element,
}

impl EncoderBin {
    fn create(encoders: &EncoderCatalog, encoder: &str, bitrate_kbps: i32) -> Result<Self, String> {
        let description = encoders.describe(encoder, bitrate_kbps);
        let bin = gst::parse::bin_from_description(&description, true)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            sink: static_pad(&bin, "sink")?,
            src: static_pad(&bin, "src")?,
            encoder: by_name(&bin, "encoder")?,
            bin,
        })
    }

    // The bin goes between the capsfilter and h264parse, which the pipeline description leaves
    // unlinked.
    fn link(&self, output_src: &gst::Pad, parse_sink: &gst::Pad) -> bool {
        output_src.link(&self.sink).is_ok() && self.src.link(parse_sink).is_ok()
    }
}

pub struct StreamSession {
    id: String,
    pipeline: gst::Pipeline,
    webrtc: gst::Element,
    capture_src: gst::Pad,
    output_src: gst::Pad,
    parse_sink: gst::Pad,
    encoder: Mutex<EncoderBin>,
    encode_timing: Arc<EncodeTiming>,
    settings: Mutex<StreamSettings>,
    // Serializes settings changes.
    updating: Mutex<()>,
    end_reason: OnceLock<String>,
    on_ended: Box<dyn Fn(&str) + Send + Sync>,
    stopped: AtomicBool,
    // Dropping the sender stops the connection monitor.
    stop_monitor: Mutex<Option<mpsc::Sender<()>>>,
    input: Arc<InputChannel>,
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

        let encoder_bin = EncoderBin::create(encoders, encoder, DEFAULT_BITRATE_KBPS)?;
        pipeline.add(&encoder_bin.bin).map_err(failed)?;
        if !encoder_bin.link(&output_src, &parse_sink) {
            return Err(failed(format!("Could not link the {encoder} encoder.")));
        }
        let encode_timing = Arc::new(EncodeTiming::default());
        encode_timing.attach(&encoder_bin.sink, &encoder_bin.src);

        let id = uuid::Uuid::new_v4().simple().to_string();
        let capture = by_name(&pipeline, "capture")?;
        let capture_src = static_pad(&capture, "src")?;
        // The viewer hides the stream cursor while it shows its own (absolute pointer mode).
        let input = InputChannel::connect(&webrtc, &id, move |visible| {
            platform::show_cursor(&capture, visible);
        });

        let session = Arc::new(Self {
            id,
            input,
            capture_src,
            output_src,
            parse_sink,
            encoder: Mutex::new(encoder_bin),
            encode_timing,
            pipeline,
            webrtc,
            settings: Mutex::new(StreamSettings {
                width: 0,
                height: 0,
                framerate: DEFAULT_FRAMERATE,
                bitrate_kbps: DEFAULT_BITRATE_KBPS,
                encoder: encoder.into(),
            }),
            updating: Mutex::new(()),
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

    pub fn encoding_stats(&self) -> EncodingStats {
        self.encode_timing.snapshot()
    }

    /// Applies the given settings while streaming. Missing fields keep their current value.
    pub fn update(
        self: &Arc<Self>,
        update: &StreamSettingsUpdate,
        encoders: &EncoderCatalog,
    ) -> Result<(), UpdateError> {
        let _updating = self.updating.lock().unwrap();
        // ViewerSessions doesn't hold its lock while updating, so the session may have been
        // stopped since it was found, e.g. by DELETE or an earlier failed switch.
        if self.stopped.load(Ordering::SeqCst) {
            return Err(UpdateError::Stopped);
        }
        let current = self.settings();
        let plan = plan_update(
            &current,
            update,
            self.source_resolution(),
            |id| encoders.is_available(id).then(|| encoders.limits(id)),
            |id| encoders.label(id).into(),
        )?;
        let next = plan.next;
        if next == current {
            return Ok(());
        }
        let encoder_changed = next.encoder != current.encoder;

        // A new encoder starts with the new bitrate. The others pick it up on their next frame.
        if plan.switch_encoder_first {
            self.swap_encoder(encoders, &next.encoder, next.bitrate_kbps)?;
        }
        if next.size() != current.size() || next.framerate != current.framerate {
            let caps = platform::output_caps(Some(next.size()), next.framerate);
            let caps = gst::Caps::from_str(&caps).map_err(|error| error.to_string())?;
            by_name(&self.pipeline, "output")?.set_property("caps", &caps);
        }
        if encoder_changed && !plan.switch_encoder_first {
            self.swap_encoder(encoders, &next.encoder, next.bitrate_kbps)?;
        } else if !encoder_changed && next.bitrate_kbps != current.bitrate_kbps {
            let encoder = &self.encoder.lock().unwrap().encoder;
            encoder.set_property("bitrate", next.bitrate_kbps as u32);
        }

        info!("Session {} settings changed to {next:?}", self.id);
        *self.settings.lock().unwrap() = next;
        Ok(())
    }

    // Swaps the encoder bin while streaming. Blocking the capsfilter's output holds the next
    // frame while the bins are exchanged; the new encoder starts with a keyframe, and rtph264pay
    // sends SPS/PPS with it, so the browser keeps decoding without renegotiating.
    fn swap_encoder(
        self: &Arc<Self>,
        encoders: &EncoderCatalog,
        encoder: &str,
        bitrate_kbps: i32,
    ) -> Result<(), UpdateError> {
        let next = Mutex::new(Some(EncoderBin::create(encoders, encoder, bitrate_kbps)?));
        let (sender, swapped) = mpsc::sync_channel(1);
        // Weak, so a probe that never fires doesn't keep the session alive.
        let session = Arc::downgrade(self);
        let encoder_id = encoder.to_owned();
        self.output_src
            .add_probe(gst::PadProbeType::BLOCK_DOWNSTREAM, move |_, _| {
                let next = next.lock().unwrap().take();
                if let (Some(session), Some(next)) = (session.upgrade(), next)
                    && !session.stopped.load(Ordering::SeqCst)
                {
                    let _ = sender.send(session.replace_encoder(next, &encoder_id));
                }
                gst::PadProbeReturn::Remove
            });

        // The probe fires on the next frame, which the capture source sends at the frame rate.
        let failure = match swapped.recv_timeout(SETUP_TIMEOUT) {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(failure)) => failure,
            Err(_) => "The encoder switch timed out.".to_owned(),
        };
        // The old encoder is gone, so there's nothing to fall back to.
        error!("Session {}: {failure}", self.id);
        self.end(failure.clone());
        Err(UpdateError::Failed(failure))
    }

    // Runs in the blocking probe, on the streaming thread.
    fn replace_encoder(&self, next: EncoderBin, encoder: &str) -> Result<(), String> {
        let mut current = self.encoder.lock().unwrap();
        let previous = std::mem::replace(&mut *current, next);
        let _ = self.output_src.unlink(&previous.sink);
        let _ = previous.src.unlink(&self.parse_sink);
        let _ = previous.bin.set_state(gst::State::Null);
        let _ = self.pipeline.remove(&previous.bin);

        self.pipeline
            .add(&current.bin)
            .map_err(|error| format!("Could not switch to the {encoder} encoder: {error}"))?;
        self.encode_timing.attach(&current.sink, &current.src);
        if !current.link(&self.output_src, &self.parse_sink)
            || current.bin.sync_state_with_parent().is_err()
        {
            return Err(format!("Could not start the {encoder} encoder."));
        }
        Ok(())
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
        self.input.release_all();
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
                self.input
                    .closed(&format!("failed: {source} association ended"));
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

fn by_name(bin: &impl IsA<gst::Bin>, name: &str) -> Result<gst::Element, String> {
    bin.by_name(name)
        .ok_or_else(|| format!("Element {name} not found."))
}

fn static_pad(element: &impl IsA<gst::Element>, name: &str) -> Result<gst::Pad, String> {
    element
        .static_pad(name)
        .ok_or_else(|| format!("Pad {name} not found."))
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
