//! Measures elapsed time from encoder input to output for matching frame timestamps.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gstreamer as gst;
use gstreamer::prelude::*;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EncodingStats {
    pub total_encode_time_ms: f64,
    pub frames_encoded: i64,
}

#[derive(Default)]
pub struct EncodeTiming {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    // Running time of each frame waiting in the encoder, and when it went in.
    pending: HashMap<u64, Instant>,
    total_encode_time_ms: f64,
    frames_encoded: i64,
}

impl EncodeTiming {
    /// Times the given encoder bin, including any upload or download it does around the encoder.
    /// Totals carry over from a previous encoder so clients' per-interval deltas stay valid.
    pub fn attach(self: &Arc<Self>, encoder_sink: &gst::Pad, encoder_src: &gst::Pad) {
        self.state.lock().unwrap().pending.clear();
        let timing = self.clone();
        encoder_sink.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
            timing.on_input(pad, info);
            gst::PadProbeReturn::Ok
        });
        let timing = self.clone();
        encoder_src.add_probe(gst::PadProbeType::BUFFER, move |pad, info| {
            timing.on_output(pad, info);
            gst::PadProbeReturn::Ok
        });
    }

    pub fn snapshot(&self) -> EncodingStats {
        let state = self.state.lock().unwrap();
        EncodingStats {
            total_encode_time_ms: state.total_encode_time_ms,
            frames_encoded: state.frames_encoded,
        }
    }

    fn on_input(&self, pad: &gst::Pad, info: &gst::PadProbeInfo) {
        let Some(running_time) = running_time(pad, info) else {
            return;
        };
        let mut state = self.state.lock().unwrap();
        // Dropped input frames have no output; bound their retained timestamps.
        if state.pending.len() >= 240 {
            state.pending.clear();
        }
        state.pending.insert(running_time, Instant::now());
    }

    fn on_output(&self, pad: &gst::Pad, info: &gst::PadProbeInfo) {
        let Some(running_time) = running_time(pad, info) else {
            return;
        };
        let mut state = self.state.lock().unwrap();
        if let Some(started) = state.pending.remove(&running_time) {
            state.total_encode_time_ms += started.elapsed().as_secs_f64() * 1000.0;
            state.frames_encoded += 1;
        }
    }
}

// GstVideoEncoder rebases output PTS. Segment running time matches both sides.
fn running_time(pad: &gst::Pad, info: &gst::PadProbeInfo) -> Option<u64> {
    let pts = info.buffer()?.pts()?;
    let event = pad.sticky_event::<gst::event::Segment>(0)?;
    let segment = event.segment().downcast_ref::<gst::ClockTime>()?;
    segment.to_running_time(pts).map(gst::ClockTime::nseconds)
}
