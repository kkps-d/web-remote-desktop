//! Gets the DRM device advertised by the GNOME compositor's default DMA-BUF feedback.

use std::env;
use std::fs;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::fs::Dev;
use rustix::net::sockopt::socket_peercred;
use wayland_client::protocol::{wl_callback, wl_registry};
use wayland_client::{Connection, Dispatch, QueueHandle, delegate_noop};
use wayland_protocols::wp::linux_dmabuf::zv1::client::{
    zwp_linux_dmabuf_feedback_v1 as feedback, zwp_linux_dmabuf_v1 as dmabuf,
};

#[derive(Default)]
struct Feedback {
    requested: bool,
    registry_done: bool,
    done: bool,
    device: Option<Dev>,
}

pub fn gnome_render_device() -> Option<Dev> {
    let display = PathBuf::from(env::var_os("WAYLAND_DISPLAY")?);
    let socket = if display.is_absolute() {
        display
    } else {
        let runtime = PathBuf::from(env::var_os("XDG_RUNTIME_DIR")?);
        runtime.is_absolute().then(|| runtime.join(display))?
    };
    let stream = UnixStream::connect(socket).ok()?;
    // A nested compositor can have a different render GPU from the GNOME portal backend.
    let pid = socket_peercred(&stream).ok()?.pid.as_raw_pid();
    if fs::read_to_string(format!("/proc/{pid}/comm")).ok()?.trim() != "gnome-shell" {
        return None;
    }
    let conn = Connection::from_socket(stream).ok()?;
    let mut queue = conn.new_event_queue();
    let handle = queue.handle();
    conn.display().get_registry(&handle, ());
    conn.display().sync(&handle, ());

    let deadline = Instant::now() + Duration::from_secs(1);
    let mut state = Feedback::default();
    loop {
        queue.dispatch_pending(&mut state).ok()?;
        if state.done {
            return state.device;
        }
        if state.registry_done && !state.requested {
            return None;
        }
        let timeout = Timespec::try_from(deadline.checked_duration_since(Instant::now())?).ok()?;
        queue.flush().ok()?;
        let Some(read) = queue.prepare_read() else {
            continue;
        };
        let mut fds = [PollFd::from_borrowed_fd(
            read.connection_fd(),
            PollFlags::IN,
        )];
        if poll(&mut fds, Some(&timeout)).ok()? == 0 {
            return None;
        }
        read.read().ok()?;
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for Feedback {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
            && interface == "zwp_linux_dmabuf_v1"
            && version >= 4
            && !state.requested
        {
            // v4 introduced default feedback. Bind it for the main_device event.
            let dmabuf = registry.bind::<dmabuf::ZwpLinuxDmabufV1, _, _>(name, 4, handle, ());
            dmabuf.get_default_feedback(handle, ());
            state.requested = true;
        }
    }
}

delegate_noop!(Feedback: ignore dmabuf::ZwpLinuxDmabufV1);

impl Dispatch<wl_callback::WlCallback, ()> for Feedback {
    fn event(
        state: &mut Self,
        _: &wl_callback::WlCallback,
        _: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        state.registry_done = true;
    }
}

impl Dispatch<feedback::ZwpLinuxDmabufFeedbackV1, ()> for Feedback {
    fn event(
        state: &mut Self,
        _: &feedback::ZwpLinuxDmabufFeedbackV1,
        event: feedback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            feedback::Event::MainDevice { device } => {
                state.device = device.try_into().ok().map(Dev::from_ne_bytes);
            }
            feedback::Event::Done => state.done = true,
            _ => (),
        }
    }
}
