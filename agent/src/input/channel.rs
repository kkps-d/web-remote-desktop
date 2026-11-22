//! Connects the viewer's "input" data channel to the OS injectors.

use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_webrtc::WebRTCDataChannel;
use tracing::{info, warn};

#[cfg(target_os = "linux")]
use super::eis::EisInput;
use super::touch::{TouchBackend, TouchState};
use super::{InputBackend, InputState, Rejected, apply};
use crate::config::CaptureConfig;
use crate::platform;

/// Receives the viewer's "input" data channel from webrtcbin and injects its messages.
pub struct InputChannel {
    session_id: String,
    backend: Backend,
}

enum Backend {
    Desktop(DesktopInput),
    #[cfg(target_os = "linux")]
    Eis(EisInput),
}

struct DesktopInput {
    input: Mutex<InputState<platform::InputInjector>>,
    touch: Mutex<TouchState<platform::TouchInjector>>,
    set_stream_cursor: Box<dyn Fn(bool) + Send + Sync>,
}

impl InputChannel {
    /// Must be connected before the offer is set, which announces the channel.
    pub fn connect(
        webrtc: &gst::Element,
        session_id: &str,
        capture: Option<&CaptureConfig>,
        set_stream_cursor: impl Fn(bool) + Send + Sync + 'static,
    ) -> Result<Arc<Self>, String> {
        let backend = match capture {
            None => Backend::Desktop(DesktopInput {
                input: Mutex::new(InputState::new(platform::InputInjector::new())),
                touch: Mutex::new(TouchState::new(platform::TouchInjector::new())),
                set_stream_cursor: Box::new(set_stream_cursor),
            }),
            #[cfg(target_os = "linux")]
            Some(capture) => {
                let path = capture
                    .eis_socket
                    .as_deref()
                    .ok_or("No EIS socket is configured")?;
                Backend::Eis(EisInput::connect(path, capture.resolution(), session_id)?)
            }
            #[cfg(not(target_os = "linux"))]
            Some(_) => return Err("Named PipeWire capture is only supported on Linux".into()),
        };
        let channel = Arc::new(Self {
            session_id: session_id.to_owned(),
            backend,
        });
        let input = channel.clone();
        webrtc.connect("on-data-channel", false, move |values| {
            if let Ok(data_channel) = values[1].get::<WebRTCDataChannel>() {
                input.on_data_channel(&data_channel);
            }
            None
        });
        Ok(channel)
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
        #[cfg(target_os = "linux")]
        if let Backend::Eis(input) = &self.backend
            && !input.attach(data_channel)
        {
            data_channel.close();
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

    fn receive(&self, text: &str) {
        match &self.backend {
            Backend::Desktop(desktop) => apply_and_log(
                text,
                &mut desktop.input.lock().unwrap(),
                Some(&mut desktop.touch.lock().unwrap()),
                &*desktop.set_stream_cursor,
                &self.session_id,
            ),
            #[cfg(target_os = "linux")]
            Backend::Eis(input) => input.receive(text),
        }
    }

    /// Releases held input after the channel closed or its SCTP association failed.
    pub fn closed(&self, reason: &str) {
        info!("Session {}: input channel {reason}", self.session_id);
        match &self.backend {
            Backend::Desktop(desktop) => desktop.release_all(),
            #[cfg(target_os = "linux")]
            Backend::Eis(input) => input.closed(&format!("The input channel {reason}")),
        }
    }

    pub fn stop(&self) {
        match &self.backend {
            Backend::Desktop(desktop) => desktop.release_all(),
            #[cfg(target_os = "linux")]
            Backend::Eis(input) => input.stop(),
        }
    }

    pub fn unavailable_reason(&self) -> Option<&str> {
        match &self.backend {
            Backend::Desktop(_) => None,
            #[cfg(target_os = "linux")]
            Backend::Eis(input) => input.reason(),
        }
    }
}

impl DesktopInput {
    fn release_all(&self) {
        self.input.lock().unwrap().release_all();
        self.touch.lock().unwrap().cancel_all();
    }
}

pub(super) fn apply_and_log<B: InputBackend, T: TouchBackend>(
    text: &str,
    input: &mut InputState<B>,
    touch: Option<&mut TouchState<T>>,
    set_stream_cursor: &dyn Fn(bool),
    session_id: &str,
) {
    match apply(text, input, touch, set_stream_cursor) {
        Ok(()) => {}
        Err(Rejected::Unknown) => warn!("Session {session_id}: unknown input message {text}"),
        Err(Rejected::Invalid(error)) => {
            warn!("Session {session_id}: invalid input message {text}: {error}");
        }
    }
}
