//! Mouse and keyboard input on the primary display with SendInput.

use ::windows::Win32::Foundation::POINT;
use ::windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use ::windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT,
    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput, VIRTUAL_KEY,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN,
};

use super::scan_codes;
use crate::input::{InputBackend, MouseButton};
use crate::touch::to_pixel;

/// Makes the process per-monitor DPI aware, so cursor positions and screen sizes are in
/// physical pixels like the captured display. Call once at startup.
pub fn use_physical_pixels() {
    // SAFETY: takes a predefined context and has no other preconditions. It fails only if the
    // awareness is already set, which then stays as it is.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// The primary display's size. In physical pixels, since the process is per-monitor DPI aware.
pub(super) fn display_size() -> (i32, i32) {
    // SAFETY: GetSystemMetrics only reads system settings.
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// SendInput's absolute coordinates for a pixel, clamped to the display. They span 0–65535 over
/// the primary display; aiming at the pixel's center keeps Windows' truncation from landing on
/// the pixel before it.
fn absolute_coordinates(x: i32, y: i32, width: i32, height: i32) -> (i32, i32) {
    let axis = |pixel: i32, size: i32| {
        ((pixel.clamp(0, size - 1) as f64 + 0.5) * 65536.0 / size as f64) as i32
    };
    (axis(x, width), axis(y, height))
}

pub struct InputInjector {
    send: Box<dyn FnMut(INPUT) + Send>,
}

impl InputInjector {
    pub fn new() -> Self {
        Self::with_sender(|input| {
            // Fails when the foreground window belongs to a more privileged process (UIPI);
            // nothing to do then.
            // SAFETY: one fully initialized INPUT, with its size.
            unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        })
    }

    /// Hands each input to `send` instead of SendInput (for tests).
    fn with_sender(send: impl FnMut(INPUT) + Send + 'static) -> Self {
        Self {
            send: Box::new(send),
        }
    }

    fn move_to_pixel(&mut self, x: i32, y: i32, width: i32, height: i32) {
        let (dx, dy) = absolute_coordinates(x, y, width, height);
        self.send_mouse(MOUSEINPUT {
            dx,
            dy,
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
            ..Default::default()
        });
    }

    fn send_mouse(&mut self, mouse: MOUSEINPUT) {
        (self.send)(INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 { mi: mouse },
        });
    }
}

impl InputBackend for InputInjector {
    fn move_to(&mut self, x: f64, y: f64) {
        let (width, height) = display_size();
        let (x, y) = to_pixel(x, y, width, height);
        self.move_to_pixel(x, y, width, height);
    }

    // Relative SendInput moves go through Windows pointer acceleration, so the delta is applied
    // to the cursor position and sent as an absolute move instead.
    fn move_by(&mut self, dx: i32, dy: i32) {
        let (width, height) = display_size();
        let mut cursor = POINT::default();
        // SAFETY: cursor is a valid POINT to write to.
        let _ = unsafe { GetCursorPos(&mut cursor) };
        self.move_to_pixel(
            cursor.x.saturating_add(dx),
            cursor.y.saturating_add(dy),
            width,
            height,
        );
    }

    fn button(&mut self, button: MouseButton, down: bool) {
        let (down_flag, up_flag) = match button {
            MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
            MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
            MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        };
        self.send_mouse(MOUSEINPUT {
            dwFlags: if down { down_flag } else { up_flag },
            ..Default::default()
        });
    }

    fn scroll(&mut self, dx: i32, dy: i32) {
        let wheel = |flags: MOUSE_EVENT_FLAGS, amount: i32| MOUSEINPUT {
            // mouseData is a signed amount stored in a DWORD.
            mouseData: amount as u32,
            dwFlags: flags,
            ..Default::default()
        };
        // Windows' vertical wheel is positive away from the user, i.e. up.
        if dy != 0 {
            self.send_mouse(wheel(MOUSEEVENTF_WHEEL, dy.saturating_neg()));
        }
        if dx != 0 {
            self.send_mouse(wheel(MOUSEEVENTF_HWHEEL, dx));
        }
    }

    fn key(&mut self, code: &str, down: bool) -> bool {
        let Some(key) = scan_codes::get(code) else {
            return false;
        };
        let mut keyboard = if key.virtual_key != 0 {
            KEYBDINPUT {
                wVk: VIRTUAL_KEY(key.virtual_key),
                ..Default::default()
            }
        } else {
            let extended = if key.extended {
                KEYEVENTF_EXTENDEDKEY
            } else {
                KEYBD_EVENT_FLAGS(0)
            };
            KEYBDINPUT {
                wScan: key.scan_code,
                dwFlags: KEYEVENTF_SCANCODE | extended,
                ..Default::default()
            }
        };
        if !down {
            keyboard.dwFlags |= KEYEVENTF_KEYUP;
        }
        (self.send)(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: keyboard },
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    // Win32 values, written out so the tests check the injector's constants too.
    const MOUSE: u32 = 0;
    const KEYBOARD: u32 = 1;
    const MOVE_ABSOLUTE: u32 = 0x8001;
    const LEFT_DOWN: u32 = 0x0002;
    const RIGHT_UP: u32 = 0x0010;
    const WHEEL: u32 = 0x0800;
    const HWHEEL: u32 = 0x1000;
    const KEY_UP: u32 = 0x0002;
    const SCAN_CODE: u32 = 0x0008;
    const EXTENDED_KEY: u32 = 0x0001;

    fn recorder() -> (InputInjector, Arc<Mutex<Vec<INPUT>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let record = sent.clone();
        let injector = InputInjector::with_sender(move |input| record.lock().unwrap().push(input));
        (injector, sent)
    }

    // (type, flags, mouseData) of each mouse input.
    fn mouse(sent: &Mutex<Vec<INPUT>>) -> Vec<(u32, u32, i32)> {
        let sent = sent.lock().unwrap();
        // SAFETY: these inputs were built as mouse inputs.
        let mouse = |input: &INPUT| unsafe { input.Anonymous.mi };
        sent.iter()
            .map(|input| {
                (
                    input.r#type.0,
                    mouse(input).dwFlags.0,
                    mouse(input).mouseData as i32,
                )
            })
            .collect()
    }

    // (type, virtual key, scan code, flags) of each keyboard input.
    fn keys(sent: &Mutex<Vec<INPUT>>) -> Vec<(u32, u16, u16, u32)> {
        let sent = sent.lock().unwrap();
        sent.iter()
            .map(|input| {
                // SAFETY: these inputs were built as keyboard inputs.
                let key = unsafe { input.Anonymous.ki };
                (input.r#type.0, key.wVk.0, key.wScan, key.dwFlags.0)
            })
            .collect()
    }

    #[test]
    fn moves_the_cursor_with_absolute_moves() {
        // Relative moves would get pointer acceleration.
        let (mut injector, sent) = recorder();
        injector.move_to(0.5, 0.5);
        injector.move_by(3, -2);

        assert_eq!(
            mouse(&sent),
            [(MOUSE, MOVE_ABSOLUTE, 0), (MOUSE, MOVE_ABSOLUTE, 0)]
        );
    }

    #[test]
    fn sends_dom_buttons_as_their_windows_buttons() {
        let (mut injector, sent) = recorder();
        injector.button(MouseButton::Left, true);
        injector.button(MouseButton::Middle, false);
        injector.button(MouseButton::Right, false);

        assert_eq!(
            mouse(&sent),
            [
                (MOUSE, LEFT_DOWN, 0),
                (MOUSE, 0x0040, 0),
                (MOUSE, RIGHT_UP, 0)
            ]
        );
    }

    #[test]
    fn scrolls_with_windows_vertical_direction_and_dom_horizontal_direction() {
        // DOM dy is positive down; Windows' wheel is positive up.
        let (mut injector, sent) = recorder();
        injector.scroll(30, 120);

        assert_eq!(mouse(&sent), [(MOUSE, WHEEL, -120), (MOUSE, HWHEEL, 30)]);
    }

    #[test]
    fn skips_scroll_axes_without_movement() {
        let (mut injector, sent) = recorder();
        injector.scroll(0, 0);
        injector.scroll(0, -120);

        assert_eq!(mouse(&sent), [(MOUSE, WHEEL, 120)]);
    }

    #[test]
    fn sends_scan_codes() {
        let (mut injector, sent) = recorder();
        injector.key("KeyA", true);
        injector.key("KeyA", false);

        assert_eq!(
            keys(&sent),
            [
                (KEYBOARD, 0, 0x1E, SCAN_CODE),
                (KEYBOARD, 0, 0x1E, SCAN_CODE | KEY_UP)
            ]
        );
    }

    #[test]
    fn marks_extended_keys() {
        let (mut injector, sent) = recorder();
        injector.key("ArrowUp", true);

        assert_eq!(keys(&sent), [(KEYBOARD, 0, 0x48, SCAN_CODE | EXTENDED_KEY)]);
    }

    #[test]
    fn sends_pause_as_a_virtual_key() {
        let (mut injector, sent) = recorder();
        injector.key("Pause", true);
        injector.key("Pause", false);

        assert_eq!(
            keys(&sent),
            [(KEYBOARD, 0x13, 0, 0), (KEYBOARD, 0x13, 0, KEY_UP)]
        );
    }

    #[test]
    fn ignores_unknown_key_codes() {
        let (mut injector, sent) = recorder();

        assert!(!injector.key("NotAKey", true));
        assert!(sent.lock().unwrap().is_empty());
    }

    #[test]
    fn aims_at_the_pixels_center_within_the_display() {
        let cases = [
            ((0, 0), (17, 30)),
            ((1919, 1079), (65518, 65505)),
            ((960, 540), (32785, 32798)),
            ((-5, -5), (17, 30)),
            ((5000, 5000), (65518, 65505)),
        ];
        for ((x, y), expected) in cases {
            assert_eq!(absolute_coordinates(x, y, 1920, 1080), expected, "{x}, {y}");
        }
    }

    #[test]
    fn every_pixel_maps_back_to_itself() {
        // Windows maps absolute coordinates to pixels as coordinate * size / 65536, truncated.
        for size in [1080, 1440, 2160, 2880] {
            for pixel in 0..size {
                let (x, _) = absolute_coordinates(pixel, 0, size, 1);
                assert_eq!(x * size / 65536, pixel, "{pixel} of {size}");
            }
        }
    }
}
