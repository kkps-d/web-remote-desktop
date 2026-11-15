//! The capture → encode → webrtcbin pipeline's description, its encoder bin, and helpers for
//! finding its elements and pads.

use gstreamer as gst;
use gstreamer::prelude::*;

use crate::encoders::EncoderCatalog;
use crate::platform;
use crate::sdp::H264Codec;
use crate::settings::{DEFAULT_FRAMERATE, Resolution, SizeLimits};

/// An encoder bin and the parts of it the session uses.
pub struct EncoderBin {
    pub bin: gst::Bin,
    pub sink: gst::Pad,
    pub src: gst::Pad,
    /// The element named "encoder", which takes bitrate changes.
    pub encoder: gst::Element,
}

impl EncoderBin {
    pub fn create(
        encoders: &EncoderCatalog,
        encoder: &str,
        bitrate_kbps: i32,
    ) -> Result<Self, String> {
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
    pub fn link(&self, output_src: &gst::Pad, parse_sink: &gst::Pad) -> bool {
        output_src.link(&self.sink).is_ok() && self.src.link(parse_sink).is_ok()
    }
}

// webrtcbin matches our stream to the offer by intersecting RTP caps exactly, so the payload
// type and profile-level-id must be the browser's. Desktop resolutions need a higher H.264 level
// than browsers offer; they accept it (level-asymmetry-allowed=1), so capssetter just relabels
// the caps. AMF labels its constrained output plain "baseline", so accept both. The encoder bin
// goes between the two chains; see StreamSession::start.
pub fn pipeline_description(
    codec: &H264Codec,
    opus_payload_type: Option<u32>,
    size: Option<Resolution>,
) -> String {
    let audio = opus_payload_type.map_or(String::new(), |payload_type| {
        format!("  {}", audio_description(payload_type))
    });
    format!(
        "{capture} ! capsfilter name=output caps={caps}  \
         h264parse name=parse ! video/x-h264,profile=(string){{constrained-baseline,baseline}} ! \
         rtph264pay pt={payload_type} config-interval=-1 aggregate-mode=zero-latency ! \
         capssetter caps=\"application/x-rtp,profile-level-id=(string){profile_level_id}\" ! \
         webrtcbin name=webrtc bundle-policy=max-bundle{audio}",
        capture = platform::CAPTURE,
        caps = platform::output_caps(size, DEFAULT_FRAMERATE),
        payload_type = codec.payload_type,
        profile_level_id = codec.profile_level_id,
    )
}

// Linked after the video, so it gets webrtcbin's sink_1 and matches the offer's audio m-line.
// Like the video, the RTP caps use the browser's payload type.
fn audio_description(payload_type: u32) -> String {
    format!(
        "{capture} ! audioconvert ! audioresample ! audio/x-raw,rate=48000,channels=2 ! \
         opusenc bitrate=128000 frame-size=10 ! rtpopuspay pt={payload_type} ! webrtc.",
        capture = platform::audio_capture(),
    )
}

// Streams start at native resolution (None), unless the display is larger than the encoder
// accepts; then they start scaled down to fit, e.g. NVENC's 4096 px on a 5K display.
pub fn initial_size(limits: SizeLimits) -> Option<Resolution> {
    let display = platform::primary_display_size()?;
    (!limits.fits(display)).then(|| limits.shrink_to_fit(display))
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

#[cfg(test)]
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

        let video = "d3d11screencapturesrc name=capture show-cursor=true ! d3d11convert ! \
             capsfilter name=output caps=video/x-raw(memory:D3D11Memory),format=NV12,width=4096,height=2304,framerate=30/1  \
             h264parse name=parse ! video/x-h264,profile=(string){constrained-baseline,baseline} ! \
             rtph264pay pt=106 config-interval=-1 aggregate-mode=zero-latency ! \
             capssetter caps=\"application/x-rtp,profile-level-id=(string)42e01f\" ! \
             webrtcbin name=webrtc bundle-policy=max-bundle";
        assert_eq!(pipeline_description(&codec, None, Some(size)), video);
        assert!(
            pipeline_description(&codec, None, None)
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
            pipeline_description(&codec, Some(111), Some(size)),
            format!("{video}  {audio}")
        );
    }
}
