//! Touch through a virtual uinput touchscreen (multitouch protocol B), which the compositor maps
//! onto the desktop and handles as real touch.

use std::io;

use evdev::uinput::VirtualDevice;
use evdev::{
    AbsInfo, AbsoluteAxisCode, AttributeSet, InputEvent, KeyCode, PropType, UinputAbsSetup,
};
use tracing::warn;

use super::input::{absolute, key};
use crate::input::touch::{MAX_CONTACTS, Phase, TouchBackend, TouchPoint};

// The touchscreen's coordinate space, which the compositor maps onto the desktop. The
// resolution (units per mm) makes it about 340 × 190 mm, a laptop-sized screen, which libinput
// uses for gesture thresholds.
const WIDTH: i32 = 19200;
const HEIGHT: i32 = 10800;
const UNITS_PER_MM: i32 = 56;

type Emit = Box<dyn FnMut(&[InputEvent]) + Send>;

pub struct TouchInjector {
    emit: Emit,
    // Bit n is set while slot n holds a contact.
    held_slots: u32,
    // Every new contact gets its own tracking id.
    next_tracking_id: i32,
}

impl TouchInjector {
    pub fn new() -> Self {
        match create_device() {
            Ok(mut device) => {
                let mut failed = false;
                Self::with_emitter(move |events| {
                    if let Err(error) = device.emit(events)
                        && !failed
                    {
                        failed = true;
                        warn!("Could not inject touch: {error}");
                    }
                })
            }
            Err(error) => {
                warn!("Touch is off: could not create a uinput touchscreen: {error}");
                Self::with_emitter(|_| {})
            }
        }
    }

    /// Hands each frame's events to `emit` instead of the device (for tests).
    fn with_emitter(emit: impl FnMut(&[InputEvent]) + Send + 'static) -> Self {
        Self {
            emit: Box::new(emit),
            held_slots: 0,
            next_tracking_id: 0,
        }
    }
}

impl TouchBackend for TouchInjector {
    fn display_size(&self) -> (i32, i32) {
        (WIDTH, HEIGHT)
    }

    // Each point updates its slot. Linux has no touch cancel, so a canceled contact lifts.
    // ABS_X and ABS_Y are declared but not sent: libinput reads the multitouch axes.
    fn send(&mut self, frame: &[TouchPoint]) {
        let was_touching = self.held_slots != 0;
        let mut events = Vec::new();
        for point in frame {
            let slot_bit = 1 << point.pointer_id;
            events.push(absolute(
                AbsoluteAxisCode::ABS_MT_SLOT,
                point.pointer_id as i32,
            ));
            match point.phase {
                Phase::Down => {
                    self.held_slots |= slot_bit;
                    events.push(absolute(
                        AbsoluteAxisCode::ABS_MT_TRACKING_ID,
                        self.next_tracking_id,
                    ));
                    self.next_tracking_id = (self.next_tracking_id + 1) % (u16::MAX as i32);
                }
                Phase::Update => {}
                Phase::Up | Phase::Canceled => {
                    self.held_slots &= !slot_bit;
                    events.push(absolute(AbsoluteAxisCode::ABS_MT_TRACKING_ID, -1));
                    continue;
                }
            }
            events.push(absolute(AbsoluteAxisCode::ABS_MT_POSITION_X, point.x));
            events.push(absolute(AbsoluteAxisCode::ABS_MT_POSITION_Y, point.y));
        }
        let touching = self.held_slots != 0;
        if touching != was_touching {
            events.push(key(KeyCode::BTN_TOUCH, touching));
        }
        (self.emit)(&events);
    }
}

fn create_device() -> io::Result<VirtualDevice> {
    let axis = |code, max| UinputAbsSetup::new(code, AbsInfo::new(0, 0, max, 0, 0, UNITS_PER_MM));
    let unitless = |code, max| UinputAbsSetup::new(code, AbsInfo::new(0, 0, max, 0, 0, 0));
    let touch: AttributeSet<KeyCode> = [KeyCode::BTN_TOUCH].into_iter().collect();
    // Direct: touches land where they are on the screen, unlike a touchpad's.
    let direct: AttributeSet<PropType> = [PropType::DIRECT].into_iter().collect();
    VirtualDevice::builder()?
        .name("Web Remote Desktop touchscreen")
        .with_keys(&touch)?
        .with_properties(&direct)?
        .with_absolute_axis(&axis(AbsoluteAxisCode::ABS_X, WIDTH - 1))?
        .with_absolute_axis(&axis(AbsoluteAxisCode::ABS_Y, HEIGHT - 1))?
        .with_absolute_axis(&unitless(
            AbsoluteAxisCode::ABS_MT_SLOT,
            MAX_CONTACTS as i32 - 1,
        ))?
        .with_absolute_axis(&unitless(
            AbsoluteAxisCode::ABS_MT_TRACKING_ID,
            u16::MAX as i32,
        ))?
        .with_absolute_axis(&axis(AbsoluteAxisCode::ABS_MT_POSITION_X, WIDTH - 1))?
        .with_absolute_axis(&axis(AbsoluteAxisCode::ABS_MT_POSITION_Y, HEIGHT - 1))?
        .build()
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use evdev::EventType;

    use super::*;

    type Sent = Arc<Mutex<Vec<Vec<(u16, u16, i32)>>>>;

    fn recording() -> (TouchInjector, Sent) {
        let sent = Sent::default();
        let log = sent.clone();
        let injector = TouchInjector::with_emitter(move |events| {
            let events = events
                .iter()
                .map(|event| (event.event_type().0, event.code(), event.value()))
                .collect();
            log.lock().unwrap().push(events);
        });
        (injector, sent)
    }

    fn abs(code: AbsoluteAxisCode, value: i32) -> (u16, u16, i32) {
        (EventType::ABSOLUTE.0, code.0, value)
    }

    fn touch(down: bool) -> (u16, u16, i32) {
        (EventType::KEY.0, KeyCode::BTN_TOUCH.0, down.into())
    }

    fn point(pointer_id: u32, x: i32, y: i32, phase: Phase) -> TouchPoint {
        TouchPoint {
            pointer_id,
            x,
            y,
            phase,
        }
    }

    #[test]
    fn lands_moves_and_lifts_contacts_in_their_slots() {
        const SLOT: AbsoluteAxisCode = AbsoluteAxisCode::ABS_MT_SLOT;
        const ID: AbsoluteAxisCode = AbsoluteAxisCode::ABS_MT_TRACKING_ID;
        const X: AbsoluteAxisCode = AbsoluteAxisCode::ABS_MT_POSITION_X;
        const Y: AbsoluteAxisCode = AbsoluteAxisCode::ABS_MT_POSITION_Y;
        let (mut injector, sent) = recording();
        injector.send(&[point(0, 10, 20, Phase::Down)]);
        injector.send(&[
            point(0, 11, 21, Phase::Update),
            point(1, 30, 40, Phase::Down),
        ]);
        injector.send(&[point(0, 11, 21, Phase::Up), point(1, 30, 40, Phase::Update)]);
        injector.send(&[point(1, 30, 40, Phase::Canceled)]);

        assert_eq!(
            *sent.lock().unwrap(),
            [
                vec![
                    abs(SLOT, 0),
                    abs(ID, 0),
                    abs(X, 10),
                    abs(Y, 20),
                    touch(true)
                ],
                vec![
                    abs(SLOT, 0),
                    abs(X, 11),
                    abs(Y, 21),
                    abs(SLOT, 1),
                    abs(ID, 1),
                    abs(X, 30),
                    abs(Y, 40)
                ],
                vec![
                    abs(SLOT, 0),
                    abs(ID, -1),
                    abs(SLOT, 1),
                    abs(X, 30),
                    abs(Y, 40)
                ],
                vec![abs(SLOT, 1), abs(ID, -1), touch(false)],
            ]
        );
    }
}
