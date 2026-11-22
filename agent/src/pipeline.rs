//! The capture → encode → webrtcbin pipeline's description, its encoder bin, and helpers for
//! finding its elements and pads.

use gstreamer as gst;
use gstreamer::prelude::*;

use crate::platform;
use crate::sdp::H264Codec;
use crate::stream_settings::Resolution;

/// An encoder bin and the parts of it the session uses.
pub struct EncoderBin {
    pub bin: gst::Bin,
    pub sink: gst::Pad,
    pub src: gst::Pad,
    /// The element named "encoder", which takes bitrate changes.
    pub encoder: gst::Element,
}

impl EncoderBin {
    pub fn create(description: &str) -> Result<Self, String> {
        let bin = gst::parse::bin_from_description(description, true)
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
    pub fn link(&self, output_src: &gst::Pad, parse_sink: &gst::Pad) -> bool {
        output_src.link(&self.sink).is_ok() && self.src.link(parse_sink).is_ok()
    }
}

const H264_BASELINE_CAPS: &str = "video/x-h264,profile=(string){constrained-baseline,baseline}";

#[cfg(target_os = "linux")]
pub fn probe_encoder(encoder: &str, size: Resolution, framerate: i32) -> Result<(), String> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let caps = platform::output_caps(Some(size), framerate);
    let pipeline = gst::parse::launch(&format!(
        "videotestsrc num-buffers=4 ! {caps},framerate={framerate}/1 ! {encoder} ! \
         h264parse ! {H264_BASELINE_CAPS} ! fakesink name=probe_sink sync=false"
    ))
    .map_err(|error| error.to_string())?
    .downcast::<gst::Pipeline>()
    .map_err(|_| "Invalid encoder probe pipeline")?;
    let received = Arc::new(AtomicBool::new(false));
    let saw_buffer = received.clone();
    static_pad(&by_name(&pipeline, "probe_sink")?, "sink")?.add_probe(
        gst::PadProbeType::BUFFER,
        move |_, _| {
            saw_buffer.store(true, Ordering::Relaxed);
            gst::PadProbeReturn::Remove
        },
    );
    let bus = pipeline
        .bus()
        .ok_or("No bus on the encoder probe pipeline")?;
    let started = pipeline.set_state(gst::State::Playing).is_ok();
    let timeout = if started {
        gst::ClockTime::from_seconds(2)
    } else {
        gst::ClockTime::ZERO
    };
    let message =
        bus.timed_pop_filtered(timeout, &[gst::MessageType::Eos, gst::MessageType::Error]);
    let stopped = pipeline.set_state(gst::State::Null);
    match message.as_ref().map(|message| message.view()) {
        Some(gst::MessageView::Error(error)) => Err(error.error().to_string()),
        Some(gst::MessageView::Eos(_)) if received.load(Ordering::Relaxed) => {
            stopped.map(|_| ()).map_err(|error| error.to_string())
        }
        Some(gst::MessageView::Eos(_)) => Err("The encoder produced no H.264 frames".into()),
        _ if !started => Err("The encoder probe could not start".into()),
        _ => Err("The encoder probe timed out".into()),
    }
}

// webrtcbin matches our stream to the offer by intersecting RTP caps exactly, so the payload
// type and profile-level-id must be the browser's. Desktop resolutions need a higher H.264 level
// than browsers offer; they accept it (level-asymmetry-allowed=1), so capssetter just relabels
// the caps. AMF labels its constrained output plain "baseline", so accept both. The encoder bin
// goes between the two chains; see StreamSession::start. `capture` is the platform's capture
// segment.
pub fn pipeline_description(
    codec: &H264Codec,
    capture: &str,
    opus_payload_type: Option<u32>,
    size: Option<Resolution>,
    framerate: i32,
) -> String {
    let audio = opus_payload_type.map_or(String::new(), |payload_type| {
        format!(
            "  {} ! webrtc.",
            audio_rtp_description(&platform::audio_capture(), payload_type)
        )
    });
    format!(
        "{capture} ! capsfilter name=output caps={caps}  \
         h264parse name=parse ! {H264_BASELINE_CAPS} ! \
         rtph264pay pt={payload_type} config-interval=-1 aggregate-mode=zero-latency ! \
         capssetter caps=\"application/x-rtp,profile-level-id=(string){profile_level_id}\" ! \
         webrtcbin name=webrtc bundle-policy=max-bundle{audio}",
        caps = platform::output_caps(size, framerate),
        payload_type = codec.payload_type,
        profile_level_id = codec.profile_level_id,
    )
}

// Like video, the RTP caps use the browser's payload type.
pub fn audio_rtp_description(capture: &str, payload_type: u32) -> String {
    format!(
        "{capture} ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! \
         opusenc bitrate=128000 frame-size=10 ! rtpopuspay pt={payload_type}"
    )
}

/// The size in a pad's caps, or 0×0 before the first frame.
pub fn caps_size(pad: &gst::Pad) -> Resolution {
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

pub fn by_name(bin: &impl IsA<gst::Bin>, name: &str) -> Result<gst::Element, String> {
    bin.by_name(name)
        .ok_or_else(|| format!("Element {name} not found."))
}

pub fn static_pad(element: &impl IsA<gst::Element>, name: &str) -> Result<gst::Pad, String> {
    element
        .static_pad(name)
        .ok_or_else(|| format!("Pad {name} not found."))
}

#[cfg(all(test, target_os = "linux"))]
#[test]
#[ignore = "requires the GStreamer x264 and H.264 parser runtime plugins"]
fn encoder_probe_requires_baseline_h264_output() {
    gst::init().unwrap();
    let size = Resolution {
        width: 320,
        height: 240,
    };
    let description = "x264enc tune=zerolatency speed-preset=ultrafast bitrate=1000";
    probe_encoder(description, size, 30).unwrap();
    let drops_input = format!("valve drop=true drop-mode=transform-to-gap ! {description}");
    assert!(probe_encoder(&drops_input, size, 30).is_err());
}

// The expected strings are the Windows capture and its D3D11 caps.
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn describes_the_capture_encode_and_webrtc_pipeline() {
        let codec = H264Codec {
            payload_type: 106,
            profile_level_id: "42e01f".into(),
        };
        let size = Resolution {
            width: 4096,
            height: 2304,
        };

        let capture = "d3d11screencapturesrc name=capture show-cursor=true ! d3d11convert";
        let video = "d3d11screencapturesrc name=capture show-cursor=true ! d3d11convert ! \
             capsfilter name=output caps=video/x-raw(memory:D3D11Memory),format=NV12,width=4096,height=2304,framerate=30/1  \
             h264parse name=parse ! video/x-h264,profile=(string){constrained-baseline,baseline} ! \
             rtph264pay pt=106 config-interval=-1 aggregate-mode=zero-latency ! \
             capssetter caps=\"application/x-rtp,profile-level-id=(string)42e01f\" ! \
             webrtcbin name=webrtc bundle-policy=max-bundle";
        assert_eq!(
            pipeline_description(&codec, capture, None, Some(size), 30),
            video
        );
        assert!(
            pipeline_description(&codec, capture, None, None, 30)
                .contains("caps=video/x-raw(memory:D3D11Memory),format=NV12,framerate=30/1  ")
        );

        let audio = format!(
            "wasapi2src loopback=true loopback-mode=exclude-process-tree loopback-target-pid={} \
             low-latency=true provide-clock=false continue-on-error=true ! audioconvert ! audioresample ! \
             audio/x-raw,rate=48000,channels=2 ! opusenc bitrate=128000 frame-size=10 ! \
             rtpopuspay pt=111 ! webrtc.",
            std::process::id()
        );
        assert_eq!(
            pipeline_description(&codec, capture, Some(111), Some(size), 30),
            format!("{video}  {audio}")
        );
    }
}
