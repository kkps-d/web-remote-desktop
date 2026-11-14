//! Touch contacts from the viewer, diffed into frames that land, move and lift each contact.
//! Each frame carries every contact on the screen, so the OS handles them as real touch
//! (pinch-zoom, inertia scrolling, press-and-hold).

use std::collections::HashSet;

use crate::input::Contact;

pub const MAX_CONTACTS: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Down,
    Update,
    Up,
    Canceled,
}

/// One contact in a frame, at a pixel of the primary display. Pointer ids run from 0 to
/// `MAX_CONTACTS - 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TouchPoint {
    pub pointer_id: u32,
    pub x: i32,
    pub y: i32,
    pub phase: Phase,
}

/// Injects touch frames into the OS.
pub trait TouchBackend: Send {
    /// The primary display's size in pixels.
    fn display_size(&self) -> (i32, i32);
    fn send(&mut self, frame: &[TouchPoint]);
}

/// The pixel at a position given as 0–1 fractions of a display, clamped to it.
pub fn to_pixel(x: f64, y: f64, width: i32, height: i32) -> (i32, i32) {
    let axis = |fraction: f64, size: i32| {
        ((fraction.clamp(0.0, 1.0) * size as f64) as i32).clamp(0, size - 1)
    };
    (axis(x, width), axis(y, height))
}

pub struct TouchState<B> {
    backend: B,
    // Viewer contact id and its held point, in the order they landed.
    held: Vec<(i32, TouchPoint)>,
}

impl<B: TouchBackend> TouchState<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            held: Vec::new(),
        }
    }

    /// Applies the contacts now on the screen, as 0–1 fractions of the display: new ids go
    /// down, known ids move, missing ids lift, or are canceled if `canceled` is set. Contacts
    /// past `MAX_CONTACTS` are ignored.
    pub fn update(&mut self, contacts: &[Contact], canceled: bool) {
        if contacts.is_empty() && self.held.is_empty() {
            return;
        }
        let (width, height) = self.backend.display_size();
        let mut frame = Vec::new();
        let mut present = HashSet::new();

        for contact in contacts {
            if !present.insert(contact.id) {
                continue;
            }
            let (x, y) = to_pixel(contact.x, contact.y, width, height);
            if let Some((_, point)) = self.held.iter_mut().find(|(id, _)| *id == contact.id) {
                (point.x, point.y, point.phase) = (x, y, Phase::Update);
                frame.push(*point);
            } else if let Some(pointer_id) = self.free_pointer_id() {
                let point = TouchPoint {
                    pointer_id,
                    x,
                    y,
                    phase: Phase::Down,
                };
                self.held.push((contact.id, point));
                frame.push(point);
            }
        }

        // Lifted after the loop, so a pointer id is never reused within the frame that lifts it.
        let phase = if canceled { Phase::Canceled } else { Phase::Up };
        self.held.retain(|(id, point)| {
            let lifted = !present.contains(id);
            if lifted {
                frame.push(TouchPoint { phase, ..*point });
            }
            !lifted
        });

        if !frame.is_empty() {
            self.backend.send(&frame);
        }
    }

    #[cfg(test)]
    pub fn into_backend(self) -> B {
        self.backend
    }

    /// Cancels every held contact, e.g. when the viewer disconnects.
    pub fn cancel_all(&mut self) {
        self.update(&[], true);
    }

    fn free_pointer_id(&self) -> Option<u32> {
        (0..MAX_CONTACTS).find(|pointer_id| {
            !self
                .held
                .iter()
                .any(|(_, point)| point.pointer_id == *pointer_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Recorder(Vec<Vec<TouchPoint>>);

    impl TouchBackend for Recorder {
        fn display_size(&self) -> (i32, i32) {
            (1920, 1080)
        }
        fn send(&mut self, frame: &[TouchPoint]) {
            self.0.push(frame.to_vec());
        }
    }

    fn touch() -> TouchState<Recorder> {
        TouchState::new(Recorder::default())
    }

    fn contacts(contacts: &[(i32, f64, f64)]) -> Vec<Contact> {
        contacts
            .iter()
            .map(|&(id, x, y)| Contact { id, x, y })
            .collect()
    }

    fn point(pointer_id: u32, phase: Phase, x: i32, y: i32) -> TouchPoint {
        TouchPoint {
            pointer_id,
            x,
            y,
            phase,
        }
    }

    use Phase::{Canceled, Down, Up, Update};

    #[test]
    fn presses_moves_and_lifts_a_contact() {
        let mut touch = touch();
        touch.update(&contacts(&[(7, 0.5, 0.5)]), false);
        touch.update(&contacts(&[(7, 0.25, 0.75)]), false);
        touch.update(&[], false);

        assert_eq!(
            touch.backend.0,
            [
                vec![point(0, Down, 960, 540)],
                vec![point(0, Update, 480, 810)],
                vec![point(0, Up, 480, 810)],
            ]
        );
    }

    #[test]
    fn sends_every_contact_in_each_frame() {
        let mut touch = touch();
        touch.update(&contacts(&[(7, 0.1, 0.1)]), false);
        touch.update(&contacts(&[(7, 0.1, 0.2), (9, 0.9, 0.9)]), false);
        touch.update(&contacts(&[(9, 0.8, 0.8)]), false);

        assert_eq!(
            touch.backend.0[1],
            [point(0, Update, 192, 216), point(1, Down, 1728, 972)]
        );
        assert_eq!(
            touch.backend.0[2],
            [point(1, Update, 1536, 864), point(0, Up, 192, 216)]
        );
    }

    #[test]
    fn reuses_a_freed_pointer_id_only_after_its_frame() {
        let mut touch = touch();
        touch.update(&contacts(&[(1, 0.0, 0.0)]), false);
        // Contact 1 lifts while contact 2 lands: 2 can't take pointer id 0 in the same frame.
        touch.update(&contacts(&[(2, 0.0, 0.0)]), false);
        touch.update(&contacts(&[(2, 0.0, 0.0), (3, 0.0, 0.0)]), false);

        assert_eq!(
            touch.backend.0[1],
            [point(1, Down, 0, 0), point(0, Up, 0, 0)]
        );
        assert_eq!(
            touch.backend.0[2],
            [point(1, Update, 0, 0), point(0, Down, 0, 0)]
        );
    }

    #[test]
    fn ignores_contacts_past_the_maximum() {
        let mut touch = touch();
        let many: Vec<_> = (0..=MAX_CONTACTS as i32).map(|id| (id, 0.5, 0.5)).collect();
        touch.update(&contacts(&many), false);

        let pointer_ids: Vec<_> = touch.backend.0[0]
            .iter()
            .map(|point| point.pointer_id)
            .collect();
        assert_eq!(pointer_ids, (0..MAX_CONTACTS).collect::<Vec<_>>());
    }

    #[test]
    fn clamps_positions_to_the_display() {
        let mut touch = touch();
        touch.update(&contacts(&[(1, -0.5, 1.5)]), false);

        assert_eq!(touch.backend.0, [vec![point(0, Down, 0, 1079)]]);
    }

    #[test]
    fn ignores_a_repeated_contact_id() {
        let mut touch = touch();
        touch.update(&contacts(&[(1, 0.5, 0.5), (1, 0.0, 0.0)]), false);

        assert_eq!(touch.backend.0, [vec![point(0, Down, 960, 540)]]);
    }

    #[test]
    fn cancels_contacts_when_asked() {
        let mut touch = touch();
        touch.update(&contacts(&[(1, 0.0, 0.0), (2, 1.0, 1.0)]), false);
        touch.update(&[], true);

        let phases: Vec<_> = touch.backend.0[1].iter().map(|point| point.phase).collect();
        assert_eq!(phases, [Canceled, Canceled]);
    }

    #[test]
    fn cancel_all_sends_nothing_without_contacts() {
        let mut touch = touch();
        touch.cancel_all();
        touch.update(&contacts(&[(1, 0.0, 0.0)]), false);
        touch.cancel_all();
        touch.cancel_all();

        assert_eq!(touch.backend.0.len(), 2);
        assert_eq!(touch.backend.0[1], [point(0, Canceled, 0, 0)]);
    }

    #[test]
    fn maps_fractions_to_pixels_within_the_display() {
        assert_eq!(to_pixel(0.5, 0.25, 1920, 1080), (960, 270));
        assert_eq!(to_pixel(1.0, 1.0, 1920, 1080), (1919, 1079));
        assert_eq!(to_pixel(-1.0, 2.0, 1920, 1080), (0, 1079));
    }
}
