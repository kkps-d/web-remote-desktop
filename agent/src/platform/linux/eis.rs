//! The libei sender for gamescope's combined virtual keyboard and pointer device.

use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::io;
use std::os::fd::{BorrowedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr::{self, NonNull};
use std::time::{Duration, Instant};

use evdev::KeyCode;
use libloading::Library;
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use super::key_codes;
use crate::input::{InputBackend, MouseButton, to_pixel};
use crate::stream_settings::Resolution;

const POINTER: c_int = 1 << 0;
const ABSOLUTE: c_int = 1 << 1;
const KEYBOARD: c_int = 1 << 2;
const SCROLL: c_int = 1 << 4;
const BUTTON: c_int = 1 << 5;
const CAPABILITIES: [c_int; 5] = [POINTER, ABSOLUTE, KEYBOARD, SCROLL, BUTTON];

const DISCONNECT: c_uint = 2;
const SEAT_ADDED: c_uint = 3;
const DEVICE_REMOVED: c_uint = 6;
const DEVICE_PAUSED: c_uint = 7;
const DEVICE_RESUMED: c_uint = 8;
const PONG: c_uint = 90;

#[repr(C)]
struct Ei {
    _opaque: [u8; 0],
}
#[repr(C)]
struct EiEvent {
    _opaque: [u8; 0],
}
#[repr(C)]
struct EiSeat {
    _opaque: [u8; 0],
}
#[repr(C)]
struct EiDevice {
    _opaque: [u8; 0],
}
#[repr(C)]
struct EiPing {
    _opaque: [u8; 0],
}

// Keep the library loaded for as long as any function pointer is in use. These signatures match
// libei.h (including the libei 1.4+ round-trip API used to flush releases during teardown).
macro_rules! libei {
    ($($name:ident: $ty:ty),+ $(,)?) => {
        struct LibEi {
            $($name: $ty,)+
            _library: Library,
        }

        impl LibEi {
            fn load() -> Result<Self, libloading::Error> {
                // SAFETY: Load the system library and retrieve its documented C ABI symbols.
                unsafe {
                    let library = Library::new("libei.so.1")?;
                    Ok(Self {
                        $($name: *library.get::<$ty>(concat!(stringify!($name), "\0").as_bytes())?,)+
                        _library: library,
                    })
                }
            }
        }
    };
}

libei! {
    ei_new_sender: unsafe extern "C" fn(*mut c_void) -> *mut Ei,
    ei_unref: unsafe extern "C" fn(*mut Ei) -> *mut Ei,
    ei_configure_name: unsafe extern "C" fn(*mut Ei, *const c_char),
    ei_setup_backend_socket: unsafe extern "C" fn(*mut Ei, *const c_char) -> c_int,
    ei_get_fd: unsafe extern "C" fn(*mut Ei) -> c_int,
    ei_dispatch: unsafe extern "C" fn(*mut Ei),
    ei_get_event: unsafe extern "C" fn(*mut Ei) -> *mut EiEvent,
    ei_event_unref: unsafe extern "C" fn(*mut EiEvent) -> *mut EiEvent,
    ei_event_get_type: unsafe extern "C" fn(*mut EiEvent) -> c_uint,
    ei_event_get_seat: unsafe extern "C" fn(*mut EiEvent) -> *mut EiSeat,
    ei_event_get_device: unsafe extern "C" fn(*mut EiEvent) -> *mut EiDevice,
    ei_seat_bind_capabilities: unsafe extern "C" fn(*mut EiSeat, ...),
    ei_device_ref: unsafe extern "C" fn(*mut EiDevice) -> *mut EiDevice,
    ei_device_unref: unsafe extern "C" fn(*mut EiDevice) -> *mut EiDevice,
    ei_device_has_capability: unsafe extern "C" fn(*mut EiDevice, c_uint) -> bool,
    ei_device_start_emulating: unsafe extern "C" fn(*mut EiDevice, u32),
    ei_device_stop_emulating: unsafe extern "C" fn(*mut EiDevice),
    ei_now: unsafe extern "C" fn(*mut Ei) -> u64,
    ei_device_frame: unsafe extern "C" fn(*mut EiDevice, u64),
    ei_device_pointer_motion_absolute: unsafe extern "C" fn(*mut EiDevice, f64, f64),
    ei_device_pointer_motion: unsafe extern "C" fn(*mut EiDevice, f64, f64),
    ei_device_button_button: unsafe extern "C" fn(*mut EiDevice, u32, bool),
    ei_device_scroll_discrete: unsafe extern "C" fn(*mut EiDevice, i32, i32),
    ei_device_keyboard_key: unsafe extern "C" fn(*mut EiDevice, u32, bool),
    ei_new_ping: unsafe extern "C" fn(*mut Ei) -> *mut EiPing,
    ei_ping: unsafe extern "C" fn(*mut EiPing),
    ei_ping_unref: unsafe extern "C" fn(*mut EiPing) -> *mut EiPing,
}

/// Owns libei's references on the worker thread.
pub struct EisConnection {
    api: LibEi,
    context: NonNull<Ei>,
    device: Option<NonNull<EiDevice>>,
    fd: RawFd,
    size: Resolution,
}

impl EisConnection {
    pub fn connect(path: &Path, size: Resolution, timeout: Duration) -> Result<Self, String> {
        let socket =
            CString::new(path.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
        let api = LibEi::load().map_err(|error| format!("Could not load libei: {error}"))?;
        // SAFETY: libei returns an owned context, or null on allocation failure.
        let context = unsafe { (api.ei_new_sender)(ptr::null_mut()) };
        let context = NonNull::new(context).ok_or("Could not create a libei sender")?;
        let mut connection = Self {
            api,
            context,
            device: None,
            fd: -1,
            size,
        };
        // SAFETY: The context is live and both strings are NUL-terminated for these calls.
        unsafe {
            (connection.api.ei_configure_name)(context.as_ptr(), c"web-remote-desktop".as_ptr());
            let status =
                (connection.api.ei_setup_backend_socket)(context.as_ptr(), socket.as_ptr());
            if status != 0 {
                return Err(io::Error::from_raw_os_error(-status).to_string());
            }
            connection.fd = (connection.api.ei_get_fd)(context.as_ptr());
        }
        if connection.fd < 0 {
            return Err("libei did not provide an event descriptor".into());
        }
        let deadline = Instant::now() + timeout;
        while connection.device.is_none() {
            if !wait_readable(connection.fd(), deadline)? {
                return Err(
                    "Timed out waiting for the gamescope keyboard and pointer device".into(),
                );
            }
            connection.dispatch()?;
        }
        Ok(connection)
    }

    pub fn fd(&self) -> BorrowedFd<'_> {
        // SAFETY: The descriptor remains open until the libei context is destroyed.
        unsafe { BorrowedFd::borrow_raw(self.fd) }
    }

    /// Drains libei's queued events. Returns true when the server acknowledges a ping.
    pub fn dispatch(&mut self) -> Result<bool, String> {
        let mut pong = false;
        // SAFETY: All pointers come from this live context. Events own their device/seat
        // references until freed. Keep a separate device reference for outbound events.
        unsafe {
            (self.api.ei_dispatch)(self.context.as_ptr());
            while let Some(event) = NonNull::new((self.api.ei_get_event)(self.context.as_ptr())) {
                let event = event.as_ptr();
                let event_type = (self.api.ei_event_get_type)(event);
                let result = match event_type {
                    SEAT_ADDED => {
                        (self.api.ei_seat_bind_capabilities)(
                            (self.api.ei_event_get_seat)(event),
                            POINTER,
                            ABSOLUTE,
                            KEYBOARD,
                            SCROLL,
                            BUTTON,
                            ptr::null::<c_void>(),
                        );
                        Ok(())
                    }
                    DEVICE_RESUMED if self.device.is_none() => {
                        let device = (self.api.ei_event_get_device)(event);
                        if CAPABILITIES
                            .iter()
                            .all(|&cap| (self.api.ei_device_has_capability)(device, cap as c_uint))
                        {
                            self.device = NonNull::new((self.api.ei_device_ref)(device));
                            (self.api.ei_device_start_emulating)(device, 1);
                        }
                        Ok(())
                    }
                    DEVICE_PAUSED | DEVICE_REMOVED
                        if self.device.is_some_and(|device| {
                            device.as_ptr() == (self.api.ei_event_get_device)(event)
                        }) =>
                    {
                        (self.api.ei_device_unref)(self.device.take().unwrap().as_ptr());
                        Err("Gamescope withdrew the input device")
                    }
                    DISCONNECT => Err("Gamescope disconnected the input socket"),
                    PONG => {
                        pong = true;
                        Ok(())
                    }
                    _ => Ok(()),
                };
                (self.api.ei_event_unref)(event);
                result?;
            }
        }
        Ok(pong)
    }

    /// Waits briefly for gamescope to process the releases before closing its connection.
    pub fn finish(&mut self) -> Result<(), String> {
        let Some(device) = self.device else {
            return Ok(());
        };
        // SAFETY: The active device and its context still own live references. libei tracks
        // the ping after ei_ping even after we drop our own reference.
        unsafe {
            (self.api.ei_device_stop_emulating)(device.as_ptr());
            let ping = (self.api.ei_new_ping)(self.context.as_ptr());
            if ping.is_null() {
                return Err("Could not allocate a libei round trip".into());
            }
            (self.api.ei_ping)(ping);
            (self.api.ei_ping_unref)(ping);
        }
        let deadline = Instant::now() + Duration::from_millis(250);
        while wait_readable(self.fd(), deadline)? {
            if self.dispatch()? {
                return Ok(());
            }
        }
        Err("Timed out waiting for gamescope to finish input emulation".into())
    }

    fn send(&self, action: impl FnOnce(&LibEi, *mut EiDevice)) {
        if let Some(device) = self.device {
            action(&self.api, device.as_ptr());
            // SAFETY: The device and its context are live and owned by this thread.
            unsafe {
                (self.api.ei_device_frame)(
                    device.as_ptr(),
                    (self.api.ei_now)(self.context.as_ptr()),
                );
            }
        }
    }
}

impl Drop for EisConnection {
    fn drop(&mut self) {
        // SAFETY: These are our own references and the library is still loaded.
        unsafe {
            if let Some(device) = self.device {
                (self.api.ei_device_unref)(device.as_ptr());
            }
            (self.api.ei_unref)(self.context.as_ptr());
        }
    }
}

impl InputBackend for EisConnection {
    fn move_to(&mut self, x: f64, y: f64) {
        let (x, y) = to_pixel(x, y, self.size.width, self.size.height);
        // SAFETY: send supplies a resumed device with this capability.
        self.send(|api, device| unsafe {
            (api.ei_device_pointer_motion_absolute)(device, x.into(), y.into());
        });
    }

    fn move_by(&mut self, dx: i32, dy: i32) {
        // SAFETY: send supplies a resumed device with this capability.
        self.send(|api, device| unsafe {
            (api.ei_device_pointer_motion)(device, dx.into(), dy.into());
        });
    }

    fn button(&mut self, button: MouseButton, down: bool) {
        let code = match button {
            MouseButton::Left => KeyCode::BTN_LEFT,
            MouseButton::Middle => KeyCode::BTN_MIDDLE,
            MouseButton::Right => KeyCode::BTN_RIGHT,
        };
        // SAFETY: send supplies a resumed device with this capability.
        self.send(|api, device| unsafe {
            (api.ei_device_button_button)(device, code.code().into(), down);
        });
    }

    fn scroll(&mut self, dx: i32, dy: i32) {
        // SAFETY: send supplies a resumed device with this capability.
        self.send(|api, device| unsafe {
            (api.ei_device_scroll_discrete)(device, dx, dy);
        });
    }

    fn key(&mut self, code: &str, down: bool) -> bool {
        let Some(key) = key_codes::get(code) else {
            return false;
        };
        // SAFETY: send supplies a resumed device with this capability.
        self.send(|api, device| unsafe {
            (api.ei_device_keyboard_key)(device, key.code().into(), down);
        });
        true
    }

    fn repeats_key_down(&self) -> bool {
        false
    }
}

fn wait_readable(fd: BorrowedFd<'_>, deadline: Instant) -> Result<bool, String> {
    let mut fds = [PollFd::new(&fd, PollFlags::IN)];
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(false);
        }
        let timeout = Timespec {
            tv_sec: remaining.as_secs() as i64,
            tv_nsec: remaining.subsec_nanos().into(),
        };
        match poll(&mut fds, Some(&timeout)) {
            Ok(ready) => return Ok(ready != 0),
            Err(Errno::INTR) => continue,
            Err(error) => return Err(error.to_string()),
        }
    }
}
