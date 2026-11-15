//! Maps `KeyboardEvent.code` (a physical key) to its scan code, so the remote keyboard layout
//! decides which character a key types.

/// A key as SendInput takes it: a scan code, or a virtual key where scan codes misbehave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub scan_code: u16,
    pub extended: bool,
    pub virtual_key: u16,
}

pub fn get(code: &str) -> Option<Key> {
    KEYS.iter()
        .find(|(name, _)| *name == code)
        .map(|(_, key)| *key)
}

const fn basic(scan_code: u16) -> Key {
    Key {
        scan_code,
        extended: false,
        virtual_key: 0,
    }
}

const fn extended(scan_code: u16) -> Key {
    Key {
        scan_code,
        extended: true,
        virtual_key: 0,
    }
}

// PS/2 set 1 scan codes; E0-prefixed keys are marked extended.
#[rustfmt::skip]
static KEYS: &[(&str, Key)] = &[
    ("Escape", basic(0x01)),
    ("Digit1", basic(0x02)), ("Digit2", basic(0x03)), ("Digit3", basic(0x04)), ("Digit4", basic(0x05)),
    ("Digit5", basic(0x06)), ("Digit6", basic(0x07)), ("Digit7", basic(0x08)), ("Digit8", basic(0x09)),
    ("Digit9", basic(0x0A)), ("Digit0", basic(0x0B)),
    ("Minus", basic(0x0C)), ("Equal", basic(0x0D)), ("Backspace", basic(0x0E)), ("Tab", basic(0x0F)),
    ("KeyQ", basic(0x10)), ("KeyW", basic(0x11)), ("KeyE", basic(0x12)), ("KeyR", basic(0x13)),
    ("KeyT", basic(0x14)), ("KeyY", basic(0x15)), ("KeyU", basic(0x16)), ("KeyI", basic(0x17)),
    ("KeyO", basic(0x18)), ("KeyP", basic(0x19)), ("BracketLeft", basic(0x1A)), ("BracketRight", basic(0x1B)),
    ("Enter", basic(0x1C)), ("ControlLeft", basic(0x1D)),
    ("KeyA", basic(0x1E)), ("KeyS", basic(0x1F)), ("KeyD", basic(0x20)), ("KeyF", basic(0x21)),
    ("KeyG", basic(0x22)), ("KeyH", basic(0x23)), ("KeyJ", basic(0x24)), ("KeyK", basic(0x25)),
    ("KeyL", basic(0x26)), ("Semicolon", basic(0x27)), ("Quote", basic(0x28)), ("Backquote", basic(0x29)),
    ("ShiftLeft", basic(0x2A)), ("Backslash", basic(0x2B)),
    ("KeyZ", basic(0x2C)), ("KeyX", basic(0x2D)), ("KeyC", basic(0x2E)), ("KeyV", basic(0x2F)),
    ("KeyB", basic(0x30)), ("KeyN", basic(0x31)), ("KeyM", basic(0x32)), ("Comma", basic(0x33)),
    ("Period", basic(0x34)), ("Slash", basic(0x35)), ("ShiftRight", basic(0x36)),
    ("NumpadMultiply", basic(0x37)), ("AltLeft", basic(0x38)), ("Space", basic(0x39)), ("CapsLock", basic(0x3A)),
    ("F1", basic(0x3B)), ("F2", basic(0x3C)), ("F3", basic(0x3D)), ("F4", basic(0x3E)), ("F5", basic(0x3F)),
    ("F6", basic(0x40)), ("F7", basic(0x41)), ("F8", basic(0x42)), ("F9", basic(0x43)), ("F10", basic(0x44)),
    ("ScrollLock", basic(0x46)),
    ("Numpad7", basic(0x47)), ("Numpad8", basic(0x48)), ("Numpad9", basic(0x49)), ("NumpadSubtract", basic(0x4A)),
    ("Numpad4", basic(0x4B)), ("Numpad5", basic(0x4C)), ("Numpad6", basic(0x4D)), ("NumpadAdd", basic(0x4E)),
    ("Numpad1", basic(0x4F)), ("Numpad2", basic(0x50)), ("Numpad3", basic(0x51)), ("Numpad0", basic(0x52)),
    ("NumpadDecimal", basic(0x53)), ("IntlBackslash", basic(0x56)), ("F11", basic(0x57)), ("F12", basic(0x58)),
    ("F13", basic(0x64)), ("F14", basic(0x65)), ("F15", basic(0x66)), ("F16", basic(0x67)), ("F17", basic(0x68)),
    ("F18", basic(0x69)), ("F19", basic(0x6A)), ("F20", basic(0x6B)), ("F21", basic(0x6C)), ("F22", basic(0x6D)),
    ("F23", basic(0x6E)), ("F24", basic(0x76)),
    ("KanaMode", basic(0x70)), ("IntlRo", basic(0x73)), ("Convert", basic(0x79)), ("NonConvert", basic(0x7B)),
    ("IntlYen", basic(0x7D)), ("NumpadComma", basic(0x7E)),

    ("MediaTrackPrevious", extended(0x10)), ("MediaTrackNext", extended(0x19)), ("NumpadEnter", extended(0x1C)),
    ("ControlRight", extended(0x1D)), ("AudioVolumeMute", extended(0x20)), ("MediaPlayPause", extended(0x22)),
    ("MediaStop", extended(0x24)), ("AudioVolumeDown", extended(0x2E)), ("AudioVolumeUp", extended(0x30)),
    ("NumpadDivide", extended(0x35)), ("PrintScreen", extended(0x37)), ("AltRight", extended(0x38)),
    // Windows reads Num Lock as E0 45; plain 45 is Pause.
    ("NumLock", extended(0x45)),
    ("Home", extended(0x47)), ("ArrowUp", extended(0x48)), ("PageUp", extended(0x49)), ("ArrowLeft", extended(0x4B)),
    ("ArrowRight", extended(0x4D)), ("End", extended(0x4F)), ("ArrowDown", extended(0x50)), ("PageDown", extended(0x51)),
    ("Insert", extended(0x52)), ("Delete", extended(0x53)), ("MetaLeft", extended(0x5B)), ("MetaRight", extended(0x5C)),
    ("ContextMenu", extended(0x5D)),

    // Pause's real scan sequence (E1 1D 45) can't be sent as one scan code, so use VK_PAUSE.
    ("Pause", Key { scan_code: 0, extended: false, virtual_key: 0x13 }),
];

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::input::STANDARD_KEY_CODES;

    #[test]
    fn maps_every_standard_key() {
        let missing: Vec<_> = STANDARD_KEY_CODES
            .iter()
            .filter(|code| get(code).is_none())
            .collect();
        assert!(missing.is_empty(), "{missing:?}");
    }

    #[test]
    fn gives_each_code_its_own_key() {
        let mut codes_by_key: HashMap<Key, Vec<&str>> = HashMap::new();
        for (code, key) in KEYS {
            codes_by_key.entry(*key).or_default().push(code);
        }
        let shared: Vec<_> = codes_by_key
            .values()
            .filter(|codes| codes.len() > 1)
            .collect();
        assert!(shared.is_empty(), "{shared:?}");
    }

    #[test]
    fn lists_each_code_once() {
        let mut codes: Vec<_> = KEYS.iter().map(|(code, _)| code).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), KEYS.len());
    }

    #[test]
    fn uses_set_1_scan_codes() {
        let cases = [
            ("KeyA", 0x1E, false),
            ("Enter", 0x1C, false),
            ("NumpadEnter", 0x1C, true),
            ("ControlRight", 0x1D, true),
            ("NumLock", 0x45, true),
            ("F24", 0x76, false),
        ];
        for (code, scan_code, extended) in cases {
            assert_eq!(
                get(code),
                Some(Key {
                    scan_code,
                    extended,
                    virtual_key: 0
                }),
                "{code}"
            );
        }
    }
}
