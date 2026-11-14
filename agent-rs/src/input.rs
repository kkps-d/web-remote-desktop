//! The viewer's messages on the "input" data channel; see the input protocol in REQUIREMENTS.md.

use serde::Deserialize;
use serde_json::Value;

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
}
