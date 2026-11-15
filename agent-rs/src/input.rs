//! The viewer's "input" data channel: its messages (see the input protocol in REQUIREMENTS.md)
//! and how they reach the OS. Held keys, buttons and touches are released when the channel
//! closes or the session ends.

pub mod touch;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_webrtc::WebRTCDataChannel;
use serde::Deserialize;
use serde_json::Value;
use tracing::{info, warn};

use crate::platform;
use touch::{TouchBackend, TouchState};

/// The pixel at a position given as 0–1 fractions of a display, clamped to it.
pub fn to_pixel(x: f64, y: f64, width: i32, height: i32) -> (i32, i32) {
    let axis = |fraction: f64, size: i32| {
        ((fraction.clamp(0.0, 1.0) * size as f64) as i32).clamp(0, size - 1)
    };
    (axis(x, width), axis(y, height))
}

/// Injects input into the OS. Positions and sizes refer to the primary display.
pub trait InputBackend: Send {
    /// Moves to a position given as 0–1 fractions of the display.
    fn move_to(&mut self, x: f64, y: f64);
    /// Moves by whole pixels, without pointer acceleration.
    fn move_by(&mut self, dx: i32, dy: i32);
    fn button(&mut self, button: MouseButton, down: bool);
    /// Scrolls by wheel units (120 per notch), positive meaning right and down like DOM deltas.
    fn scroll(&mut self, dx: i32, dy: i32);
    /// Presses or releases a key by its `KeyboardEvent.code`. Returns false, sending nothing,
    /// for a code it doesn't know.
    fn key(&mut self, code: &str, down: bool) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    /// From a DOM button number; other buttons aren't supported.
    fn from_dom(button: i32) -> Option<Self> {
        match button {
            0 => Some(Self::Left),
            1 => Some(Self::Middle),
            2 => Some(Self::Right),
            _ => None,
        }
    }
}

/// Remembers what is held, so it can be released when the viewer goes away.
pub struct InputState<B> {
    backend: B,
    held_buttons: HashSet<MouseButton>,
    held_keys: HashSet<String>,
}

impl<B: InputBackend> InputState<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            held_buttons: HashSet::new(),
            held_keys: HashSet::new(),
        }
    }

    fn button(&mut self, button: i32, down: bool) {
        let Some(button) = MouseButton::from_dom(button) else {
            return;
        };
        if down {
            self.held_buttons.insert(button);
        } else if !self.held_buttons.remove(&button) {
            return;
        }
        self.backend.button(button, down);
    }

    fn key(&mut self, code: &str, down: bool) {
        // A repeated down is an auto-repeat and is sent again.
        if down {
            if self.backend.key(code, true) {
                self.held_keys.insert(code.to_owned());
            }
        } else if self.held_keys.remove(code) {
            self.backend.key(code, false);
        }
    }

    /// Releases every held key and button, e.g. when the viewer disconnects.
    pub fn release_all(&mut self) {
        for code in self.held_keys.drain() {
            self.backend.key(&code, false);
        }
        for button in self.held_buttons.drain() {
            self.backend.button(button, false);
        }
    }
}

/// Why a message wasn't applied.
#[derive(Debug)]
enum Rejected {
    Unknown,
    Invalid(serde_json::Error),
}

/// Applies one message from the viewer.
fn apply<B: InputBackend, T: TouchBackend>(
    text: &str,
    input: &mut InputState<B>,
    touch: &mut TouchState<T>,
    set_stream_cursor: &dyn Fn(bool),
) -> Result<(), Rejected> {
    match parse(text).map_err(Rejected::Invalid)? {
        InputMessage::Move { x, y } => input.backend.move_to(x, y),
        InputMessage::MoveBy { dx, dy } => input.backend.move_by(dx, dy),
        InputMessage::Button { button, down } => input.button(button, down),
        InputMessage::Wheel { dx, dy } => input.backend.scroll(dx, dy),
        InputMessage::Key { code, down } => input.key(&code, down),
        InputMessage::Touch { contacts, canceled } => touch.update(&contacts, canceled),
        InputMessage::ReleaseAll => {
            input.release_all();
            touch.cancel_all();
        }
        InputMessage::Cursor { visible } => set_stream_cursor(visible),
        InputMessage::Unknown => return Err(Rejected::Unknown),
    }
    Ok(())
}

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

/// One message. Numbers other than `x` and `y` must be integers.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum InputMessage {
    /// Absolute position, 0–1 of the video content area.
    Move {
        x: f64,
        y: f64,
    },
    /// Relative move in pixels.
    MoveBy {
        dx: i32,
        dy: i32,
    },
    /// DOM button number: 0 left, 1 middle, 2 right.
    Button {
        button: i32,
        down: bool,
    },
    /// Windows wheel units (120 per notch), positive right and down like DOM deltas.
    Wheel {
        dx: i32,
        dy: i32,
    },
    /// `KeyboardEvent.code`.
    Key {
        code: String,
        down: bool,
    },
    /// Every finger on the stream; missing ids lift, or are canceled with `canceled`.
    Touch {
        contacts: Vec<Contact>,
        #[serde(default)]
        canceled: bool,
    },
    ReleaseAll,
    /// Shows or hides the cursor in the stream.
    Cursor {
        visible: bool,
    },
    /// A type this agent doesn't know, e.g. from a newer client.
    #[serde(other)]
    Unknown,
}

/// A finger on the stream, at 0–1 fractions of the video content area like `Move`.
#[derive(Debug, PartialEq, Deserialize)]
pub struct Contact {
    pub id: i32,
    pub x: f64,
    pub y: f64,
}

/// Parses one message; an error means it is malformed.
pub fn parse(text: &str) -> serde_json::Result<InputMessage> {
    let message: Value = serde_json::from_str(text)?;
    // The C# agent reads a null type as an unknown one, but serde's tag must be a string.
    if message.get("type").is_some_and(Value::is_null) {
        return Ok(InputMessage::Unknown);
    }
    InputMessage::deserialize(message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use touch::{Phase, TouchPoint};

    #[test]
    fn maps_fractions_to_pixels_within_the_display() {
        assert_eq!(to_pixel(0.5, 0.25, 1920, 1080), (960, 270));
        assert_eq!(to_pixel(1.0, 1.0, 1920, 1080), (1919, 1079));
        assert_eq!(to_pixel(-1.0, 2.0, 1920, 1080), (0, 1079));
    }

    #[test]
    fn parses_every_message_type() {
        let cases = [
            (
                r#"{"type":"move","x":0.5,"y":1}"#,
                InputMessage::Move { x: 0.5, y: 1.0 },
            ),
            (
                r#"{"type":"moveBy","dx":3,"dy":-2}"#,
                InputMessage::MoveBy { dx: 3, dy: -2 },
            ),
            (
                r#"{"type":"button","button":0,"down":true}"#,
                InputMessage::Button {
                    button: 0,
                    down: true,
                },
            ),
            (
                r#"{"type":"wheel","dx":0,"dy":120}"#,
                InputMessage::Wheel { dx: 0, dy: 120 },
            ),
            (
                r#"{"type":"key","code":"KeyA","down":false}"#,
                InputMessage::Key {
                    code: "KeyA".into(),
                    down: false,
                },
            ),
            (r#"{"type":"releaseAll"}"#, InputMessage::ReleaseAll),
            (
                r#"{"type":"cursor","visible":true}"#,
                InputMessage::Cursor { visible: true },
            ),
        ];
        for (text, message) in cases {
            assert_eq!(parse(text).unwrap(), message, "{text}");
        }
    }

    #[test]
    fn parses_touch_frames() {
        let frame = parse(
            r#"{"type":"touch","contacts":[{"id":3,"x":0.5,"y":0.25},{"id":4,"x":1,"y":1}]}"#,
        );
        let contacts = vec![
            Contact {
                id: 3,
                x: 0.5,
                y: 0.25,
            },
            Contact {
                id: 4,
                x: 1.0,
                y: 1.0,
            },
        ];
        assert_eq!(
            frame.unwrap(),
            InputMessage::Touch {
                contacts,
                canceled: false
            }
        );

        let canceled = parse(r#"{"type":"touch","contacts":[],"canceled":true}"#);
        assert_eq!(
            canceled.unwrap(),
            InputMessage::Touch {
                contacts: vec![],
                canceled: true
            }
        );
    }

    #[test]
    fn parses_unknown_types() {
        for text in [r#"{"type":"scroll"}"#, r#"{"type":null}"#] {
            assert_eq!(parse(text).unwrap(), InputMessage::Unknown, "{text}");
        }
    }

    #[test]
    fn rejects_malformed_messages() {
        let malformed = [
            "",
            "not json",
            r#"{"type":"key","code":"KeyA","down":true"#,
            "[]",
            "42",
            "{}",
            r#"{"type":5}"#,
            r#"{"type":"move","x":0.5}"#,
            r#"{"type":"move","x":"left","y":0.5}"#,
            r#"{"type":"moveBy","dx":1.5,"dy":0}"#,
            r#"{"type":"moveBy","dx":1e12,"dy":0}"#,
            r#"{"type":"button","button":0,"down":"yes"}"#,
            r#"{"type":"key","code":null,"down":true}"#,
            r#"{"type":"cursor","visible":1}"#,
            r#"{"type":"touch"}"#,
            r#"{"type":"touch","contacts":{}}"#,
            r#"{"type":"touch","contacts":[{"id":1,"x":"left","y":0.5}]}"#,
            r#"{"type":"touch","contacts":[{"id":1.5,"x":0.5,"y":0.5}]}"#,
            r#"{"type":"touch","contacts":[],"canceled":"yes"}"#,
        ];
        for text in malformed {
            assert!(parse(text).is_err(), "{text}");
        }
    }

    #[derive(Debug, PartialEq)]
    enum Sent {
        MoveTo(f64, f64),
        MoveBy(i32, i32),
        Button(MouseButton, bool),
        Scroll(i32, i32),
        Key(String, bool),
    }

    /// Records what would be injected. Knows every key code except "NotAKey".
    #[derive(Default)]
    struct Recorder(Vec<Sent>);

    impl InputBackend for Recorder {
        fn move_to(&mut self, x: f64, y: f64) {
            self.0.push(Sent::MoveTo(x, y));
        }
        fn move_by(&mut self, dx: i32, dy: i32) {
            self.0.push(Sent::MoveBy(dx, dy));
        }
        fn button(&mut self, button: MouseButton, down: bool) {
            self.0.push(Sent::Button(button, down));
        }
        fn scroll(&mut self, dx: i32, dy: i32) {
            self.0.push(Sent::Scroll(dx, dy));
        }
        fn key(&mut self, code: &str, down: bool) -> bool {
            let known = code != "NotAKey";
            if known {
                self.0.push(Sent::Key(code.into(), down));
            }
            known
        }
    }

    /// Records touch frames on a 1920×1080 display.
    #[derive(Default)]
    struct TouchRecorder(Vec<Vec<TouchPoint>>);

    impl TouchBackend for TouchRecorder {
        fn display_size(&self) -> (i32, i32) {
            (1920, 1080)
        }
        fn send(&mut self, frame: &[TouchPoint]) {
            self.0.push(frame.to_vec());
        }
    }

    fn input() -> InputState<Recorder> {
        InputState::new(Recorder::default())
    }

    fn touch() -> TouchState<TouchRecorder> {
        TouchState::new(TouchRecorder::default())
    }

    // Applies the messages in order and returns what was sent.
    fn receive(messages: &[&str]) -> Vec<Sent> {
        let mut input = input();
        for text in messages {
            let _ = apply(text, &mut input, &mut touch(), &|_| {});
        }
        input.backend.0
    }

    // Applies the messages in order and returns the touch frames' phases and the mouse and
    // keyboard input sent.
    fn receive_touch(messages: &[&str]) -> (Vec<Vec<TouchPoint>>, Vec<Sent>) {
        let (mut input, mut touch) = (input(), touch());
        for text in messages {
            apply(text, &mut input, &mut touch, &|_| {}).unwrap();
        }
        (touch.into_backend().0, input.backend.0)
    }

    #[test]
    fn applies_touch_frames() {
        let (frames, sent) = receive_touch(&[
            r#"{"type":"touch","contacts":[{"id":3,"x":0.5,"y":0.25},{"id":4,"x":1,"y":1}]}"#,
            r#"{"type":"touch","contacts":[],"canceled":true}"#,
        ]);

        let phases =
            |frame: &[TouchPoint]| frame.iter().map(|point| point.phase).collect::<Vec<_>>();
        assert_eq!((frames[0][0].x, frames[0][0].y), (960, 270));
        assert_eq!(phases(&frames[0]), [Phase::Down, Phase::Down]);
        assert_eq!(phases(&frames[1]), [Phase::Canceled, Phase::Canceled]);
        assert!(sent.is_empty());
    }

    #[test]
    fn release_all_cancels_touches() {
        let (frames, _) = receive_touch(&[
            r#"{"type":"touch","contacts":[{"id":1,"x":0.5,"y":0.5}]}"#,
            r#"{"type":"releaseAll"}"#,
        ]);

        assert_eq!(
            frames[1]
                .iter()
                .map(|point| point.phase)
                .collect::<Vec<_>>(),
            [Phase::Canceled]
        );
    }

    #[test]
    fn applies_input_messages() {
        let cases = [
            (r#"{"type":"move","x":0.5,"y":0.5}"#, Sent::MoveTo(0.5, 0.5)),
            (r#"{"type":"moveBy","dx":3,"dy":-2}"#, Sent::MoveBy(3, -2)),
            (
                r#"{"type":"button","button":0,"down":true}"#,
                Sent::Button(MouseButton::Left, true),
            ),
            (r#"{"type":"wheel","dx":0,"dy":120}"#, Sent::Scroll(0, 120)),
            (
                r#"{"type":"key","code":"KeyA","down":true}"#,
                Sent::Key("KeyA".into(), true),
            ),
        ];
        for (text, sent) in cases {
            assert_eq!(receive(&[text]), [sent], "{text}");
        }
    }

    #[test]
    fn release_all_releases_held_input() {
        let sent = receive(&[
            r#"{"type":"key","code":"ControlLeft","down":true}"#,
            r#"{"type":"releaseAll"}"#,
        ]);

        assert_eq!(sent.last(), Some(&Sent::Key("ControlLeft".into(), false)));
    }

    #[test]
    fn sets_the_stream_cursor() {
        for visible in [true, false] {
            let cursor = Mutex::new(Vec::new());
            let text = format!(r#"{{"type":"cursor","visible":{visible}}}"#);
            apply(&text, &mut input(), &mut touch(), &|visible| {
                cursor.lock().unwrap().push(visible)
            })
            .unwrap();

            assert_eq!(cursor.into_inner().unwrap(), [visible]);
        }
    }

    #[test]
    fn rejects_unknown_types() {
        let mut input = input();
        let result = apply(r#"{"type":"scroll"}"#, &mut input, &mut touch(), &|_| {});

        assert!(matches!(result, Err(Rejected::Unknown)));
        assert!(input.backend.0.is_empty());
    }

    #[test]
    fn rejects_malformed_messages_and_keeps_applying_input() {
        let mut input = input();
        let rejected = apply(
            r#"{"type":"move","x":0.5}"#,
            &mut input,
            &mut touch(),
            &|_| {},
        );
        apply(
            r#"{"type":"button","button":0,"down":true}"#,
            &mut input,
            &mut touch(),
            &|_| {},
        )
        .unwrap();

        assert!(matches!(rejected, Err(Rejected::Invalid(_))));
        assert_eq!(input.backend.0, [Sent::Button(MouseButton::Left, true)]);
    }

    #[test]
    fn sends_a_button_up_only_for_a_held_button() {
        let mut input = input();
        input.button(0, false);
        input.button(0, true);
        input.button(0, false);
        input.button(0, false);

        assert_eq!(
            input.backend.0,
            [
                Sent::Button(MouseButton::Left, true),
                Sent::Button(MouseButton::Left, false)
            ]
        );
    }

    #[test]
    fn ignores_buttons_other_than_left_middle_and_right() {
        for button in [-1, 3] {
            let mut input = input();
            input.button(button, true);
            input.release_all();

            assert!(input.backend.0.is_empty(), "{button}");
        }
    }

    #[test]
    fn sends_repeated_key_downs_and_one_up() {
        let mut input = input();
        input.key("KeyA", true);
        input.key("KeyA", true);
        input.key("KeyA", false);
        input.key("KeyA", false);

        let down = || Sent::Key("KeyA".into(), true);
        assert_eq!(
            input.backend.0,
            [down(), down(), Sent::Key("KeyA".into(), false)]
        );
    }

    #[test]
    fn ignores_unknown_key_codes() {
        let mut input = input();
        input.key("NotAKey", true);
        input.release_all();

        assert!(input.backend.0.is_empty());
    }

    #[test]
    fn release_all_releases_everything_held_once() {
        let mut input = input();
        input.key("ShiftLeft", true);
        input.button(2, true);
        input.button(1, true);
        input.button(1, false);
        input.backend.0.clear();

        input.release_all();
        input.release_all();
        input.key("ShiftLeft", false);
        input.button(2, false);

        assert_eq!(
            input.backend.0,
            [
                Sent::Key("ShiftLeft".into(), false),
                Sent::Button(MouseButton::Right, false)
            ]
        );
    }
}
