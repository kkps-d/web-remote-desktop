//! Connects the viewer's "input" data channel to the OS injectors.

use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_webrtc::WebRTCDataChannel;
use tracing::{info, warn};

use super::touch::TouchState;
use super::{InputState, Rejected, apply};
use crate::platform;

/// Receives the viewer's "input" data channel from webrtcbin and injects its messages.
pub struct InputChannel {
    session_id: String,
    input: Mutex<InputState<platform::InputInjector>>,
    touch: Mutex<TouchState<platform::TouchInjector>>,
    set_stream_cursor: Box<dyn Fn(bool) + Send + Sync>,
}

impl InputChannel {
    /// Must be connected before the offer is set, which announces the channel.
    pub fn connect(
        webrtc: &gst::Element,
        session_id: &str,
        set_stream_cursor: impl Fn(bool) + Send + Sync + 'static,
    ) -> Arc<Self> {
        let channel = Arc::new(Self {
            session_id: session_id.to_owned(),
            input: Mutex::new(InputState::new(platform::InputInjector::new())),
            touch: Mutex::new(TouchState::new(platform::TouchInjector::new())),
            set_stream_cursor: Box::new(set_stream_cursor),
        });
        let input = channel.clone();
        webrtc.connect("on-data-channel", false, move |values| {
            if let Ok(data_channel) = values[1].get::<WebRTCDataChannel>() {
                input.on_data_channel(&data_channel);
            }
            None
        });
        channel
    }

    fn on_data_channel(self: &Arc<Self>, data_channel: &WebRTCDataChannel) {
        let label = data_channel.label().unwrap_or_default();
        if label != "input" {
            warn!(
                "Session {}: ignoring data channel \"{label}\"",
                self.session_id
            );
            return;
        }
        // webrtcbin owns the channel, and the handlers go with it.
        let input = self.clone();
        data_channel.connect_on_open(move |_| {
            info!("Session {}: input channel open", input.session_id);
        });
        let input = self.clone();
        data_channel.connect_on_close(move |_| input.closed("closed"));
        let input = self.clone();
        data_channel.connect_on_message_string(move |_, text| input.receive(text.unwrap_or("")));
    }

    // A bad message is logged and dropped, so a viewer can't make the agent fail.
    fn receive(&self, text: &str) {
        let mut input = self.input.lock().unwrap();
        let mut touch = self.touch.lock().unwrap();
        match apply(text, &mut input, &mut touch, &*self.set_stream_cursor) {
            Ok(()) => {}
            Err(Rejected::Unknown) => {
                warn!("Session {}: unknown input message {text}", self.session_id);
            }
            Err(Rejected::Invalid(error)) => {
                warn!(
                    "Session {}: invalid input message {text}: {error}",
                    self.session_id
                );
            }
        }
    }

    /// Releases held input after the channel closed or failed, e.g. its SCTP association ended.
    pub fn closed(&self, reason: &str) {
        info!("Session {}: input channel {reason}", self.session_id);
        self.release_all();
    }

    /// Releases held input. Call after the pipeline has stopped, so no more messages arrive.
    pub fn release_all(&self) {
        self.input.lock().unwrap().release_all();
        self.touch.lock().unwrap().cancel_all();
    }
}
