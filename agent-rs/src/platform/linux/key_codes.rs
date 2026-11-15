//! Maps `KeyboardEvent.code` (a physical key) to its evdev key code, so the desktop's keyboard
//! layout decides which character a key types.

use evdev::KeyCode;

pub fn get(code: &str) -> Option<KeyCode> {
    KEYS.iter()
        .find(|(name, _)| *name == code)
        .map(|(_, key)| *key)
}

/// Every key the table maps, for the virtual keyboard to declare.
pub fn all() -> impl Iterator<Item = KeyCode> {
    KEYS.iter().map(|(_, key)| *key)
}

#[rustfmt::skip]
static KEYS: &[(&str, KeyCode)] = &[
    ("Escape", KeyCode::KEY_ESC),
    ("Digit1", KeyCode::KEY_1), ("Digit2", KeyCode::KEY_2), ("Digit3", KeyCode::KEY_3), ("Digit4", KeyCode::KEY_4),
    ("Digit5", KeyCode::KEY_5), ("Digit6", KeyCode::KEY_6), ("Digit7", KeyCode::KEY_7), ("Digit8", KeyCode::KEY_8),
    ("Digit9", KeyCode::KEY_9), ("Digit0", KeyCode::KEY_0),
    ("Minus", KeyCode::KEY_MINUS), ("Equal", KeyCode::KEY_EQUAL), ("Backspace", KeyCode::KEY_BACKSPACE),
    ("Tab", KeyCode::KEY_TAB),
    ("KeyQ", KeyCode::KEY_Q), ("KeyW", KeyCode::KEY_W), ("KeyE", KeyCode::KEY_E), ("KeyR", KeyCode::KEY_R),
    ("KeyT", KeyCode::KEY_T), ("KeyY", KeyCode::KEY_Y), ("KeyU", KeyCode::KEY_U), ("KeyI", KeyCode::KEY_I),
    ("KeyO", KeyCode::KEY_O), ("KeyP", KeyCode::KEY_P),
    ("BracketLeft", KeyCode::KEY_LEFTBRACE), ("BracketRight", KeyCode::KEY_RIGHTBRACE),
    ("Enter", KeyCode::KEY_ENTER), ("ControlLeft", KeyCode::KEY_LEFTCTRL),
    ("KeyA", KeyCode::KEY_A), ("KeyS", KeyCode::KEY_S), ("KeyD", KeyCode::KEY_D), ("KeyF", KeyCode::KEY_F),
    ("KeyG", KeyCode::KEY_G), ("KeyH", KeyCode::KEY_H), ("KeyJ", KeyCode::KEY_J), ("KeyK", KeyCode::KEY_K),
    ("KeyL", KeyCode::KEY_L),
    ("Semicolon", KeyCode::KEY_SEMICOLON), ("Quote", KeyCode::KEY_APOSTROPHE), ("Backquote", KeyCode::KEY_GRAVE),
    ("ShiftLeft", KeyCode::KEY_LEFTSHIFT), ("Backslash", KeyCode::KEY_BACKSLASH),
    ("KeyZ", KeyCode::KEY_Z), ("KeyX", KeyCode::KEY_X), ("KeyC", KeyCode::KEY_C), ("KeyV", KeyCode::KEY_V),
    ("KeyB", KeyCode::KEY_B), ("KeyN", KeyCode::KEY_N), ("KeyM", KeyCode::KEY_M),
    ("Comma", KeyCode::KEY_COMMA), ("Period", KeyCode::KEY_DOT), ("Slash", KeyCode::KEY_SLASH),
    ("ShiftRight", KeyCode::KEY_RIGHTSHIFT),
    ("AltLeft", KeyCode::KEY_LEFTALT), ("Space", KeyCode::KEY_SPACE), ("CapsLock", KeyCode::KEY_CAPSLOCK),
    ("AltRight", KeyCode::KEY_RIGHTALT), ("ControlRight", KeyCode::KEY_RIGHTCTRL),
    ("MetaLeft", KeyCode::KEY_LEFTMETA), ("MetaRight", KeyCode::KEY_RIGHTMETA),
    ("ContextMenu", KeyCode::KEY_COMPOSE),

    ("F1", KeyCode::KEY_F1), ("F2", KeyCode::KEY_F2), ("F3", KeyCode::KEY_F3), ("F4", KeyCode::KEY_F4),
    ("F5", KeyCode::KEY_F5), ("F6", KeyCode::KEY_F6), ("F7", KeyCode::KEY_F7), ("F8", KeyCode::KEY_F8),
    ("F9", KeyCode::KEY_F9), ("F10", KeyCode::KEY_F10), ("F11", KeyCode::KEY_F11), ("F12", KeyCode::KEY_F12),
    ("F13", KeyCode::KEY_F13), ("F14", KeyCode::KEY_F14), ("F15", KeyCode::KEY_F15), ("F16", KeyCode::KEY_F16),
    ("F17", KeyCode::KEY_F17), ("F18", KeyCode::KEY_F18), ("F19", KeyCode::KEY_F19), ("F20", KeyCode::KEY_F20),
    ("F21", KeyCode::KEY_F21), ("F22", KeyCode::KEY_F22), ("F23", KeyCode::KEY_F23), ("F24", KeyCode::KEY_F24),

    ("PrintScreen", KeyCode::KEY_SYSRQ), ("ScrollLock", KeyCode::KEY_SCROLLLOCK), ("Pause", KeyCode::KEY_PAUSE),
    ("Insert", KeyCode::KEY_INSERT), ("Home", KeyCode::KEY_HOME), ("PageUp", KeyCode::KEY_PAGEUP),
    ("Delete", KeyCode::KEY_DELETE), ("End", KeyCode::KEY_END), ("PageDown", KeyCode::KEY_PAGEDOWN),
    ("ArrowUp", KeyCode::KEY_UP), ("ArrowLeft", KeyCode::KEY_LEFT), ("ArrowDown", KeyCode::KEY_DOWN),
    ("ArrowRight", KeyCode::KEY_RIGHT),

    ("NumLock", KeyCode::KEY_NUMLOCK), ("NumpadDivide", KeyCode::KEY_KPSLASH),
    ("NumpadMultiply", KeyCode::KEY_KPASTERISK), ("NumpadSubtract", KeyCode::KEY_KPMINUS),
    ("NumpadAdd", KeyCode::KEY_KPPLUS), ("NumpadEnter", KeyCode::KEY_KPENTER), ("NumpadDecimal", KeyCode::KEY_KPDOT),
    ("NumpadComma", KeyCode::KEY_KPCOMMA),
    ("Numpad0", KeyCode::KEY_KP0), ("Numpad1", KeyCode::KEY_KP1), ("Numpad2", KeyCode::KEY_KP2),
    ("Numpad3", KeyCode::KEY_KP3), ("Numpad4", KeyCode::KEY_KP4), ("Numpad5", KeyCode::KEY_KP5),
    ("Numpad6", KeyCode::KEY_KP6), ("Numpad7", KeyCode::KEY_KP7), ("Numpad8", KeyCode::KEY_KP8),
    ("Numpad9", KeyCode::KEY_KP9),

    ("IntlBackslash", KeyCode::KEY_102ND), ("IntlRo", KeyCode::KEY_RO), ("IntlYen", KeyCode::KEY_YEN),
    ("KanaMode", KeyCode::KEY_KATAKANAHIRAGANA), ("Convert", KeyCode::KEY_HENKAN),
    ("NonConvert", KeyCode::KEY_MUHENKAN),

    ("AudioVolumeMute", KeyCode::KEY_MUTE), ("AudioVolumeDown", KeyCode::KEY_VOLUMEDOWN),
    ("AudioVolumeUp", KeyCode::KEY_VOLUMEUP), ("MediaPlayPause", KeyCode::KEY_PLAYPAUSE),
    ("MediaStop", KeyCode::KEY_STOPCD), ("MediaTrackPrevious", KeyCode::KEY_PREVIOUSSONG),
    ("MediaTrackNext", KeyCode::KEY_NEXTSONG),
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
        let mut codes_by_key: HashMap<KeyCode, Vec<&str>> = HashMap::new();
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
    fn maps_keys_whose_evdev_names_differ() {
        let cases = [
            ("Quote", KeyCode::KEY_APOSTROPHE),
            ("PrintScreen", KeyCode::KEY_SYSRQ),
            ("ContextMenu", KeyCode::KEY_COMPOSE),
            ("IntlBackslash", KeyCode::KEY_102ND),
            ("NumpadEnter", KeyCode::KEY_KPENTER),
        ];
        for (code, key) in cases {
            assert_eq!(get(code), Some(key), "{code}");
        }
    }
}
