//! Optional audio from a specific PipeWire sink monitor.

#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

use gstreamer as gst;
use gstreamer::prelude::*;

use crate::pipeline::{audio_rtp_description, by_name, static_pad};

const SOURCE: &str = "pipewiresrc name=game_audio_source provide-clock=false on-disconnect=error \
    stream-properties=\"props,stream.capture.sink=true,node.dont-fallback=true,node.dont-reconnect=true,node.dont-move=true\" \
    ! audio/x-raw,rate=48000,channels=2";

pub struct NamedAudio {
    pub name: String,
    bin: gst::Bin,
    src: gst::Pad,
    sink: Option<gst::Pad>,
    stop_worker: Option<Sender<()>>,
    worker: Option<JoinHandle<()>>,
    received_rtp: Arc<AtomicBool>,
    pub keep_pad: bool,
}

impl NamedAudio {
    pub fn new(name: &str, payload_type: u32) -> Result<Self, String> {
        let description = audio_rtp_description(SOURCE, payload_type);
        let bin = gst::parse::bin_from_description_with_name(&description, true, "game_audio")
            .map_err(|error| error.to_string())?;
        by_name(&bin, "game_audio_source")?.set_property("target-object", name);
        let src = static_pad(&bin, "src")?;
        // PipeWire flushes the source when a sink disappears. Keep that flush (and any EOS)
        // inside the audio bin so it cannot reach webrtcbin's shared audio/video transport.
        src.add_probe(
            gst::PadProbeType::EVENT_DOWNSTREAM | gst::PadProbeType::EVENT_FLUSH,
            |_, info| match info.event().map(|event| event.type_()) {
                Some(
                    gst::EventType::FlushStart | gst::EventType::FlushStop | gst::EventType::Eos,
                ) => gst::PadProbeReturn::Drop,
                _ => gst::PadProbeReturn::Ok,
            },
        );
        Ok(Self {
            name: name.into(),
            src,
            bin,
            sink: None,
            stop_worker: None,
            worker: None,
            received_rtp: Arc::new(AtomicBool::new(false)),
            keep_pad: false,
        })
    }

    pub fn start(
        &mut self,
        pipeline: &gst::Pipeline,
        webrtc: &gst::Element,
    ) -> Result<gst::Pad, String> {
        // pipewiresrc shares its PipeWire connection with other sources using the same fd.
        // Keep audio on a different connection so losing its sink cannot break video.
        #[cfg(target_os = "linux")]
        let remote = {
            let remote = crate::platform::open_pipewire_socket()?;
            by_name(&self.bin, "game_audio_source")?.set_property("fd", remote.as_raw_fd());
            remote
        };
        // PipeWire may block in its state transition while waiting for a link. Keep the
        // optional bin out of parent state changes and let its worker own both transitions.
        self.bin.set_locked_state(true);
        pipeline.add(&self.bin).map_err(|error| error.to_string())?;
        let sink = webrtc
            .request_pad_simple("sink_1")
            .ok_or("Could not request the WebRTC audio pad")?;
        self.sink = Some(sink.clone());
        self.src.link(&sink).map_err(|error| error.to_string())?;
        let received_rtp = self.received_rtp.clone();
        self.src.add_probe(
            gst::PadProbeType::BUFFER | gst::PadProbeType::BUFFER_LIST,
            move |_, _| {
                received_rtp.store(true, Ordering::Relaxed);
                gst::PadProbeReturn::Remove
            },
        );
        let (stop, stopping) = mpsc::channel();
        let bin = self.bin.clone();
        self.worker = Some(
            thread::Builder::new()
                .name("game-audio".into())
                .spawn(move || {
                    #[cfg(target_os = "linux")]
                    let _remote = remote;
                    // The bus watcher reports the source's error rather than a generic sync error.
                    let _ = bin.sync_state_with_parent();
                    let _ = stopping.recv();
                    let _ = bin.set_state(gst::State::Null);
                })
                .map_err(|error| error.to_string())?,
        );
        self.stop_worker = Some(stop);
        Ok(sink)
    }

    pub fn ready(&self, pad: &gst::Pad) -> bool {
        self.received_rtp.load(Ordering::Relaxed) && pad.current_caps().is_some()
    }

    pub fn owns(&self, source: &gst::Object) -> bool {
        source == self.bin.upcast_ref::<gst::Object>() || source.has_as_ancestor(&self.bin)
    }

    pub fn stop(&mut self, webrtc: &gst::Element) {
        if let Some(sink) = self.sink.take() {
            let _ = self.src.unlink(&sink);
            // Keep the negotiated transceiver when the source disappears during playback.
            if !self.keep_pad {
                webrtc.release_request_pad(&sink);
            }
        }
        self.stop_worker.take();
    }

    /// Ensure the bin has reached NULL before its parent changes state during teardown.
    pub fn join(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[test]
#[ignore = "requires GStreamer's PipeWire and Opus plugins"]
fn uses_only_the_configured_sink_monitor() {
    gst::init().unwrap();
    let audio = NamedAudio::new("game.audio", 111).unwrap();
    let source = by_name(&audio.bin, "game_audio_source").unwrap();
    assert!(audio.owns(source.upcast_ref()));
    assert_eq!(source.property::<String>("target-object"), "game.audio");
    assert!(!source.property::<bool>("provide-clock"));
    let properties = source.property::<gst::Structure>("stream-properties");
    assert!(properties.get::<bool>("stream.capture.sink").unwrap());
    assert!(properties.get::<bool>("node.dont-fallback").unwrap());
    assert!(properties.get::<bool>("node.dont-reconnect").unwrap());
    assert!(properties.get::<bool>("node.dont-move").unwrap());
}

#[cfg(all(test, target_os = "linux"))]
#[test]
#[ignore = "requires GStreamer's PipeWire and Opus plugins"]
fn audio_flush_and_eos_do_not_reach_the_transport() {
    use std::sync::Mutex;

    gst::init().unwrap();
    let audio = NamedAudio::new("game.audio", 111).unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = gst::Pad::builder(gst::PadDirection::Sink)
        .event_function({
            let seen = seen.clone();
            move |_, _, event| {
                seen.lock().unwrap().push(event.type_());
                true
            }
        })
        .build();
    sink.set_active(true).unwrap();
    audio.src.set_active(true).unwrap();
    audio
        .src
        .link_full(&sink, gst::PadLinkCheck::empty())
        .unwrap();

    assert!(audio.src.push_event(gst::event::StreamStart::new("test")));
    audio.src.push_event(gst::event::FlushStart::new());
    audio.src.push_event(gst::event::FlushStop::new(false));
    audio.src.push_event(gst::event::Eos::new());

    assert_eq!(*seen.lock().unwrap(), [gst::EventType::StreamStart]);
}
