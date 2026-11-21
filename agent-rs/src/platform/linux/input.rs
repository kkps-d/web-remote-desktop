//! Mouse and keyboard input through uinput: virtual devices that the compositor handles like
//! real ones. Needs write access to /dev/uinput; see docs/LINUX_DEV.md.

use std::io;

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode,
    UinputAbsSetup,
};
use tracing::warn;

use super::key_codes;
use crate::input::{InputBackend, MouseButton};

// The absolute pointer's range, which the compositor maps onto the desktop.
const POINTER_MAX: i32 = 65535;
const WHEEL_NOTCH: i32 = 120;

/// The virtual device an event goes to. Absolute and relative motion need separate devices;
/// buttons and the wheel ride on the absolute one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Keyboard,
    Pointer,
    Mouse,
}

type Emit = Box<dyn FnMut(Target, &[InputEvent]) + Send>;

pub struct InputInjector {
    emit: Emit,
    // Wheel units (x, y) not yet sent as whole notches.
    wheel_remainder: (i32, i32),
}

impl InputInjector {
    pub fn new() -> Self {
        match Devices::create() {
            Ok(mut devices) => {
                let mut failed = false;
                Self::with_emitter(move |target, events| {
                    if let Err(error) = devices.get(target).emit(events)
                        && !failed
                    {
                        failed = true;
                        warn!("Could not inject input: {error}");
                    }
                })
            }
            Err(error) => {
                warn!("Input is off: could not create uinput devices: {error}");
                Self::with_emitter(|_, _| {})
            }
        }
    }

    /// Hands each batch of events to `emit` instead of the devices (for tests).
    fn with_emitter(emit: impl FnMut(Target, &[InputEvent]) + Send + 'static) -> Self {
        Self {
            emit: Box::new(emit),
            wheel_remainder: (0, 0),
        }
    }
}

impl InputBackend for InputInjector {
    fn move_to(&mut self, x: f64, y: f64) {
        let axis = |fraction: f64| (fraction.clamp(0.0, 1.0) * POINTER_MAX as f64).round() as i32;
        (self.emit)(
            Target::Pointer,
            &[
                absolute(AbsoluteAxisCode::ABS_X, axis(x)),
                absolute(AbsoluteAxisCode::ABS_Y, axis(y)),
            ],
        );
    }

    // The compositor applies its pointer acceleration, as for a real mouse. Games that lock the
    // pointer read the unaccelerated motion.
    fn move_by(&mut self, dx: i32, dy: i32) {
        (self.emit)(
            Target::Mouse,
            &[
                relative(RelativeAxisCode::REL_X, dx),
                relative(RelativeAxisCode::REL_Y, dy),
            ],
        );
    }

    fn button(&mut self, button: MouseButton, down: bool) {
        let code = match button {
            MouseButton::Left => KeyCode::BTN_LEFT,
            MouseButton::Middle => KeyCode::BTN_MIDDLE,
            MouseButton::Right => KeyCode::BTN_RIGHT,
        };
        (self.emit)(Target::Pointer, &[key(code, down)]);
    }

    // High-resolution wheel events use the same 120 units per notch as the viewer. The classic
    // events follow once a whole notch has added up, for apps that only read those. Linux's
    // vertical wheel is positive away from the user, i.e. up.
    fn scroll(&mut self, dx: i32, dy: i32) {
        let dy = dy.saturating_neg();
        let mut events = Vec::new();
        let (remainder_x, remainder_y) = &mut self.wheel_remainder;
        for (amount, remainder, high_resolution, notches) in [
            (
                dx,
                remainder_x,
                RelativeAxisCode::REL_HWHEEL_HI_RES,
                RelativeAxisCode::REL_HWHEEL,
            ),
            (
                dy,
                remainder_y,
                RelativeAxisCode::REL_WHEEL_HI_RES,
                RelativeAxisCode::REL_WHEEL,
            ),
        ] {
            if amount == 0 {
                continue;
            }
            events.push(relative(high_resolution, amount));
            *remainder = remainder.saturating_add(amount);
            let whole = *remainder / WHEEL_NOTCH;
            if whole != 0 {
                *remainder -= whole * WHEEL_NOTCH;
                events.push(relative(notches, whole));
            }
        }
        if !events.is_empty() {
            (self.emit)(Target::Pointer, &events);
        }
    }

    // The kernel drops a repeated press of a held key; the compositor repeats keys itself.
    fn key(&mut self, code: &str, down: bool) -> bool {
        let Some(code) = key_codes::get(code) else {
            return false;
        };
        (self.emit)(Target::Keyboard, &[key(code, down)]);
        true
    }
}

struct Devices {
    keyboard: VirtualDevice,
    pointer: VirtualDevice,
    mouse: VirtualDevice,
}

impl Devices {
    fn create() -> io::Result<Self> {
        let buttons: AttributeSet<KeyCode> =
            [KeyCode::BTN_LEFT, KeyCode::BTN_RIGHT, KeyCode::BTN_MIDDLE]
                .into_iter()
                .collect();
        let keys: AttributeSet<KeyCode> = key_codes::all().collect();
        let wheel: AttributeSet<RelativeAxisCode> = [
            RelativeAxisCode::REL_WHEEL,
            RelativeAxisCode::REL_HWHEEL,
            RelativeAxisCode::REL_WHEEL_HI_RES,
            RelativeAxisCode::REL_HWHEEL_HI_RES,
        ]
        .into_iter()
        .collect();
        let motion: AttributeSet<RelativeAxisCode> =
            [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y]
                .into_iter()
                .collect();
        let pointer_axis =
            |code| UinputAbsSetup::new(code, AbsInfo::new(0, 0, POINTER_MAX, 0, 0, 0));

        Ok(Self {
            keyboard: VirtualDevice::builder()?
                .name("Web Remote Desktop keyboard")
                .with_keys(&keys)?
                .build()?,
            pointer: VirtualDevice::builder()?
                .name("Web Remote Desktop pointer")
                .with_keys(&buttons)?
                .with_absolute_axis(&pointer_axis(AbsoluteAxisCode::ABS_X))?
                .with_absolute_axis(&pointer_axis(AbsoluteAxisCode::ABS_Y))?
                .with_relative_axes(&wheel)?
                .build()?,
            // Buttons too, or the device isn't recognized as a mouse.
            mouse: VirtualDevice::builder()?
                .name("Web Remote Desktop relative mouse")
                .with_keys(&buttons)?
                .with_relative_axes(&motion)?
                .build()?,
        })
    }

    fn get(&mut self, target: Target) -> &mut VirtualDevice {
        match target {
            Target::Keyboard => &mut self.keyboard,
            Target::Pointer => &mut self.pointer,
            Target::Mouse => &mut self.mouse,
        }
    }
}

pub(super) fn key(code: KeyCode, down: bool) -> InputEvent {
    InputEvent::new(EventType::KEY.0, code.0, down.into())
}

pub(super) fn absolute(code: AbsoluteAxisCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::ABSOLUTE.0, code.0, value)
}

fn relative(code: RelativeAxisCode, value: i32) -> InputEvent {
    InputEvent::new(EventType::RELATIVE.0, code.0, value)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    type Sent = Arc<Mutex<Vec<(Target, Vec<(u16, u16, i32)>)>>>;

    fn recording() -> (InputInjector, Sent) {
        let sent = Sent::default();
        let log = sent.clone();
        let injector = InputInjector::with_emitter(move |target, events| {
            let events = events
                .iter()
                .map(|event| (event.event_type().0, event.code(), event.value()))
                .collect();
            log.lock().unwrap().push((target, events));
        });
        (injector, sent)
    }

    fn rel(code: RelativeAxisCode, value: i32) -> (u16, u16, i32) {
        (EventType::RELATIVE.0, code.0, value)
    }

    #[test]
    fn moves_the_absolute_pointer_across_its_range() {
        let (mut injector, sent) = recording();
        injector.move_to(0.5, 1.5);
        let abs = |code: AbsoluteAxisCode, value| (EventType::ABSOLUTE.0, code.0, value);
        assert_eq!(
            *sent.lock().unwrap(),
            [(
                Target::Pointer,
                vec![
                    abs(AbsoluteAxisCode::ABS_X, 32768),
                    abs(AbsoluteAxisCode::ABS_Y, POINTER_MAX)
                ]
            )]
        );
    }

    #[test]
    fn scrolls_in_high_resolution_and_adds_up_notches() {
        let (mut injector, sent) = recording();
        // Down by half a notch twice, then right by a notch.
        injector.scroll(0, 60);
        injector.scroll(0, 60);
        injector.scroll(120, 0);
        assert_eq!(
            *sent.lock().unwrap(),
            [
                (
                    Target::Pointer,
                    vec![rel(RelativeAxisCode::REL_WHEEL_HI_RES, -60)]
                ),
                (
                    Target::Pointer,
                    vec![
                        rel(RelativeAxisCode::REL_WHEEL_HI_RES, -60),
                        rel(RelativeAxisCode::REL_WHEEL, -1)
                    ]
                ),
                (
                    Target::Pointer,
                    vec![
                        rel(RelativeAxisCode::REL_HWHEEL_HI_RES, 120),
                        rel(RelativeAxisCode::REL_HWHEEL, 1)
                    ]
                ),
            ]
        );
    }

    #[test]
    fn sends_keys_and_buttons_to_their_devices() {
        let (mut injector, sent) = recording();
        assert!(injector.key("KeyA", true));
        assert!(!injector.key("NoSuchKey", true));
        injector.button(MouseButton::Right, false);
        injector.move_by(3, -4);
        let key = |code: KeyCode, value| (EventType::KEY.0, code.0, value);
        assert_eq!(
            *sent.lock().unwrap(),
            [
                (Target::Keyboard, vec![key(KeyCode::KEY_A, 1)]),
                (Target::Pointer, vec![key(KeyCode::BTN_RIGHT, 0)]),
                (
                    Target::Mouse,
                    vec![
                        rel(RelativeAxisCode::REL_X, 3),
                        rel(RelativeAxisCode::REL_Y, -4)
                    ]
                ),
            ]
        );
    }
}
