//! Touch on the primary display through a synthetic touch device, which Windows handles as real
//! touch. Needs Windows 10 1809 or later.

use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use ::windows::Win32::Foundation::{POINT, RECT};
use ::windows::Win32::UI::Controls::{
    CreateSyntheticPointerDevice, DestroySyntheticPointerDevice, HSYNTHETICPOINTERDEVICE,
    POINTER_FEEDBACK_DEFAULT, POINTER_TYPE_INFO, POINTER_TYPE_INFO_0,
};
use ::windows::Win32::UI::Input::Pointer::{
    InjectSyntheticPointerInput, POINTER_FLAG_CANCELED, POINTER_FLAG_DOWN, POINTER_FLAG_INCONTACT,
    POINTER_FLAG_INRANGE, POINTER_FLAG_UP, POINTER_FLAG_UPDATE, POINTER_INFO, POINTER_TOUCH_INFO,
};
use ::windows::Win32::UI::WindowsAndMessaging::{PT_TOUCH, TOUCH_MASK_CONTACTAREA};
use tracing::warn;

use crate::input::touch::{MAX_CONTACTS, Phase, TouchBackend, TouchPoint};

// Windows cancels a contact that gets no updates for a while, so held contacts are resent.
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_millis(100);
// Windows accepts contacts without a contact area but never delivers them to windows.
const CONTACT_RADIUS: i32 = 4;

type Inject = Box<dyn FnMut(&[POINTER_TYPE_INFO]) + Send>;

/// Injects each frame as it arrives, so touch keeps its order with mouse and keyboard input. A
/// keep-alive thread resends held contacts while they're still; dropping the injector stops it,
/// so nothing is injected afterwards.
pub struct TouchInjector {
    injection: Arc<Mutex<Injection>>,
    // Each frame restarts the keep-alive countdown; dropping the sender stops the thread.
    frame_sent: Option<mpsc::Sender<()>>,
    keep_alive: Option<JoinHandle<()>>,
}

struct Injection {
    inject: Inject,
    held: HeldContacts,
}

impl TouchInjector {
    pub fn new() -> Self {
        let mut device = Device::default();
        Self::with_injector(
            KEEP_ALIVE_INTERVAL,
            Box::new(move |frame| device.inject(frame)),
        )
    }

    /// Injects with `inject` instead of the synthetic device (for tests).
    fn with_injector(keep_alive_interval: Duration, inject: Inject) -> Self {
        let injection = Arc::new(Mutex::new(Injection {
            inject,
            held: HeldContacts::default(),
        }));
        let (frame_sent, frames_sent) = mpsc::channel();
        let keep_alive = {
            let injection = injection.clone();
            thread::Builder::new()
                .name("touch keep-alive".into())
                .spawn(move || keep_alive(&injection, &frames_sent, keep_alive_interval))
                .expect("could not start the touch keep-alive thread")
        };
        Self {
            injection,
            frame_sent: Some(frame_sent),
            keep_alive: Some(keep_alive),
        }
    }
}

impl TouchBackend for TouchInjector {
    fn display_size(&self) -> (i32, i32) {
        super::display_size()
    }

    fn send(&mut self, frame: &[TouchPoint]) {
        let mut injection = self.injection.lock().unwrap();
        let infos = injection.held.next(frame);
        (injection.inject)(&infos);
        drop(injection);
        if let Some(frame_sent) = &self.frame_sent {
            let _ = frame_sent.send(());
        }
    }
}

impl Drop for TouchInjector {
    fn drop(&mut self) {
        self.frame_sent.take();
        if let Some(keep_alive) = self.keep_alive.take() {
            let _ = keep_alive.join();
        }
    }
}

// Resends held contacts after each keep-alive interval without a frame.
fn keep_alive(injection: &Mutex<Injection>, frames_sent: &Receiver<()>, interval: Duration) {
    loop {
        let holding = !injection.lock().unwrap().held.0.is_empty();
        let next = if holding {
            frames_sent.recv_timeout(interval)
        } else {
            frames_sent
                .recv()
                .map_err(|_| RecvTimeoutError::Disconnected)
        };
        match next {
            // A frame went out, which restarts the countdown.
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => {
                let mut injection = injection.lock().unwrap();
                let infos = injection.held.resend();
                if !infos.is_empty() {
                    (injection.inject)(&infos);
                }
            }
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// The contacts on the screen, for the keep-alive.
#[derive(Default)]
struct HeldContacts(Vec<TouchPoint>);

impl HeldContacts {
    /// The frame as pointer infos. Every frame carries all contacts on the screen, so it also
    /// says which are held.
    fn next(&mut self, frame: &[TouchPoint]) -> Vec<POINTER_TYPE_INFO> {
        self.0 = frame
            .iter()
            .filter(|point| matches!(point.phase, Phase::Down | Phase::Update))
            .map(|point| TouchPoint {
                phase: Phase::Update,
                ..*point
            })
            .collect();
        frame.iter().map(pointer_info).collect()
    }

    /// The held contacts again, where they are.
    fn resend(&self) -> Vec<POINTER_TYPE_INFO> {
        self.0.iter().map(pointer_info).collect()
    }
}

fn pointer_info(point: &TouchPoint) -> POINTER_TYPE_INFO {
    let in_contact = POINTER_FLAG_INRANGE | POINTER_FLAG_INCONTACT;
    let flags = match point.phase {
        Phase::Down => POINTER_FLAG_DOWN | in_contact,
        Phase::Update => POINTER_FLAG_UPDATE | in_contact,
        Phase::Up => POINTER_FLAG_UP,
        Phase::Canceled => POINTER_FLAG_UP | POINTER_FLAG_CANCELED,
    };
    let (x, y) = (point.x, point.y);
    POINTER_TYPE_INFO {
        r#type: PT_TOUCH,
        Anonymous: POINTER_TYPE_INFO_0 {
            touchInfo: POINTER_TOUCH_INFO {
                pointerInfo: POINTER_INFO {
                    pointerType: PT_TOUCH,
                    pointerId: point.pointer_id,
                    pointerFlags: flags,
                    ptPixelLocation: POINT { x, y },
                    ..Default::default()
                },
                touchMask: TOUCH_MASK_CONTACTAREA,
                rcContact: RECT {
                    left: x - CONTACT_RADIUS,
                    top: y - CONTACT_RADIUS,
                    right: x + CONTACT_RADIUS,
                    bottom: y + CONTACT_RADIUS,
                },
                ..Default::default()
            },
        },
    }
}

/// The synthetic touch device, created on the first touch so sessions without touch don't get
/// one.
#[derive(Default)]
struct Device {
    handle: Option<HSYNTHETICPOINTERDEVICE>,
    failed: bool,
}

// SAFETY: the handle isn't tied to the thread that created it; the C# agent injected through it
// from both the message thread and timer threads. The injector's mutex serializes its use.
unsafe impl Send for Device {}

impl Device {
    fn inject(&mut self, frame: &[POINTER_TYPE_INFO]) {
        if self.handle.is_none() && !self.failed {
            // SAFETY: plain values; the handle is destroyed in Drop.
            match unsafe {
                CreateSyntheticPointerDevice(PT_TOUCH, MAX_CONTACTS, POINTER_FEEDBACK_DEFAULT)
            } {
                Ok(handle) => self.handle = Some(handle),
                Err(error) => {
                    self.failed = true;
                    warn!(
                        "Couldn't create a touch device ({error}); touch input is ignored from now on."
                    );
                }
            }
        }
        if let Some(handle) = self.handle {
            // Fails when the foreground window belongs to a more privileged process (UIPI), like
            // SendInput; nothing to do then.
            // SAFETY: a live device handle and fully initialized pointer infos.
            let _ = unsafe { InjectSyntheticPointerInput(handle, frame) };
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        if let Some(handle) = self.handle {
            // SAFETY: the handle came from CreateSyntheticPointerDevice and is destroyed once.
            unsafe { DestroySyntheticPointerDevice(handle) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Win32 values, written out so the tests check the injector's constants too.
    const TOUCH_POINTER: i32 = 2;
    const DOWN: u32 = 0x10000 | 0x2 | 0x4;
    const UPDATE: u32 = 0x20000 | 0x2 | 0x4;
    const UP: u32 = 0x40000;
    const CANCELED: u32 = 0x40000 | 0x8000;

    fn point(phase: Phase) -> TouchPoint {
        TouchPoint {
            pointer_id: 3,
            x: 960,
            y: 540,
            phase,
        }
    }

    // (pointer id, flags, x, y) of each pointer info.
    type Described = Vec<(u32, u32, i32, i32)>;

    fn describe(infos: &[POINTER_TYPE_INFO]) -> Described {
        infos
            .iter()
            .map(|info| {
                // SAFETY: these infos were built as touch infos.
                let pointer = unsafe { info.Anonymous.touchInfo.pointerInfo };
                (
                    pointer.pointerId,
                    pointer.pointerFlags.0,
                    pointer.ptPixelLocation.x,
                    pointer.ptPixelLocation.y,
                )
            })
            .collect()
    }

    #[test]
    fn matches_the_win32_struct_sizes() {
        assert_eq!(size_of::<POINTER_INFO>(), 96);
        assert_eq!(size_of::<POINTER_TOUCH_INFO>(), 144);
        assert_eq!(size_of::<POINTER_TYPE_INFO>(), 152);
    }

    #[test]
    fn describes_each_phase_as_a_touch_pointer() {
        let phases = [Phase::Down, Phase::Update, Phase::Up, Phase::Canceled];
        let infos: Vec<_> = phases
            .iter()
            .map(|&phase| pointer_info(&point(phase)))
            .collect();

        assert_eq!(
            describe(&infos),
            [
                (3, DOWN, 960, 540),
                (3, UPDATE, 960, 540),
                (3, UP, 960, 540),
                (3, CANCELED, 960, 540)
            ]
        );
        for info in &infos {
            // SAFETY: built as a touch info.
            let pointer_type = unsafe { info.Anonymous.touchInfo.pointerInfo.pointerType };
            assert_eq!(
                (info.r#type.0, pointer_type.0),
                (TOUCH_POINTER, TOUCH_POINTER)
            );
        }
    }

    #[test]
    fn gives_every_contact_a_contact_area() {
        // SAFETY: built as a touch info.
        let touch = unsafe { pointer_info(&point(Phase::Down)).Anonymous.touchInfo };
        let contact = touch.rcContact;

        assert_eq!(touch.touchMask, 0x1);
        assert_eq!(
            (contact.left, contact.top, contact.right, contact.bottom),
            (956, 536, 964, 544)
        );
    }

    #[test]
    fn keep_alive_resends_held_contacts_only() {
        let mut held = HeldContacts::default();
        assert!(held.resend().is_empty());

        held.next(&[point(Phase::Down)]);
        assert_eq!(describe(&held.resend()), [(3, UPDATE, 960, 540)]);

        held.next(&[point(Phase::Up)]);
        assert!(held.resend().is_empty());
    }

    fn recording_injector(interval: Duration) -> (TouchInjector, Receiver<Described>) {
        let (sender, injected) = mpsc::channel();
        let touch = TouchInjector::with_injector(
            interval,
            Box::new(move |frame| {
                let _ = sender.send(describe(frame));
            }),
        );
        (touch, injected)
    }

    #[test]
    fn injects_each_frame_before_send_returns() {
        let (mut touch, injected) = recording_injector(Duration::from_secs(60));
        touch.send(&[point(Phase::Down)]);

        assert_eq!(injected.try_recv().unwrap(), [(3, DOWN, 960, 540)]);
    }

    #[test]
    fn keep_alive_timer_resends_held_contacts() {
        let (mut touch, injected) = recording_injector(Duration::from_millis(10));
        touch.send(&[point(Phase::Down)]);

        // The press itself, then at least one keep-alive.
        let timeout = Duration::from_secs(5);
        assert_eq!(
            injected.recv_timeout(timeout).unwrap(),
            [(3, DOWN, 960, 540)]
        );
        assert_eq!(
            injected.recv_timeout(timeout).unwrap(),
            [(3, UPDATE, 960, 540)]
        );
    }

    #[test]
    fn injects_nothing_after_being_dropped() {
        let (mut touch, injected) = recording_injector(Duration::from_millis(1));
        touch.send(&[point(Phase::Down)]);
        drop(touch);
        let sent_before_drop = injected.try_iter().count();

        thread::sleep(Duration::from_millis(50));
        assert!(sent_before_drop >= 1);
        // The sender went with the injector, so the channel reports it closed, with nothing new.
        assert!(matches!(
            injected.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
