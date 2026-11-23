use std::cell::RefCell;
use std::env;
use std::io::Write;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use gstreamer::glib::WeakRef;
use gstreamer::prelude::ObjectExt;
use gstreamer_webrtc::WebRTCDataChannel;
use rustix::event::{EventfdFlags, PollFd, PollFlags, eventfd, poll};
use rustix::io::{self, Errno};
use rustix::net::{AddressFamily, SocketAddrUnix, SocketFlags, SocketType, connect, socket_with};
use tracing::warn;

use super::InputState;
use super::channel::apply_and_log;
use crate::platform::{EisConnection, TouchInjector};
use crate::stream_settings::Resolution;

const DEVICE_TIMEOUT: Duration = Duration::from_secs(2);
const QUEUE_SIZE: usize = 256;

pub struct EisInput {
    messages: SyncSender<String>,
    shared: Arc<Shared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

struct Shared {
    wake: OwnedFd,
    stopping: AtomicBool,
    reason: OnceLock<String>,
    channel: Mutex<Option<WeakRef<WebRTCDataChannel>>>,
}

impl EisInput {
    pub fn connect(
        socket: &Path,
        cursor_socket: Option<&Path>,
        size: Resolution,
        session_id: &str,
    ) -> Result<Self, String> {
        let path = socket_path(socket).map_err(|error| format!("EIS socket: {error}"))?;
        let cursor_socket = cursor_socket.map(Path::to_owned);
        let shared = Arc::new(Shared {
            wake: eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
                .map_err(|error| error.to_string())?,
            stopping: AtomicBool::new(false),
            reason: OnceLock::new(),
            channel: Mutex::new(None),
        });
        let (messages, incoming) = mpsc::sync_channel(QUEUE_SIZE);
        let (ready, startup) = mpsc::channel();
        let session_id = session_id.to_owned();
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("gamescope-input".into())
            .spawn(move || {
                let connection = match EisConnection::connect(&path, size, DEVICE_TIMEOUT) {
                    Ok(connection) => connection,
                    Err(error) => {
                        let _ = ready.send(Err(format!("EIS socket {}: {error}", path.display())));
                        return;
                    }
                };
                if ready.send(Ok(())).is_ok() {
                    let cursor = cursor_socket.as_deref().and_then(|socket| {
                        connect_cursor(socket)
                            .map_err(|error| warn!("Session {session_id}: {error}"))
                            .ok()
                    });
                    run(connection, cursor, incoming, &worker_shared, &session_id);
                }
            })
            .map_err(|error| error.to_string())?;
        let input = Self {
            messages,
            shared,
            worker: Mutex::new(Some(worker)),
        };
        startup.recv().map_err(|error| error.to_string())??;
        Ok(input)
    }

    pub fn receive(&self, text: &str) {
        if self.shared.stopping.load(Ordering::Acquire) || text.len() > 4096 {
            return;
        }
        match self.messages.try_send(text.to_owned()) {
            Ok(()) => self.shared.wake(),
            Err(TrySendError::Full(_)) => {
                let reason = "The gamescope input queue filled up";
                if self.shared.reason.set(reason.into()).is_ok() {
                    warn!("{reason}");
                }
                self.shared.stop();
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }

    /// One ordered channel per session. If the worker has already failed, close it now.
    pub fn attach(&self, channel: &WebRTCDataChannel) -> bool {
        let mut current = self.shared.channel.lock().unwrap();
        if current.is_some() || self.shared.stopping.load(Ordering::Acquire) {
            return false;
        }
        *current = Some(channel.downgrade());
        true
    }

    pub fn reason(&self) -> Option<&str> {
        self.shared.reason.get().map(String::as_str)
    }

    pub fn closed(&self, reason: &str) {
        let _ = self.shared.reason.set(reason.into());
        self.shared.stop();
    }

    pub fn stop(&self) {
        self.shared.stop();
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

impl Drop for EisInput {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Shared {
    fn wake(&self) {
        // A full eventfd already has a pending wakeup.
        while matches!(
            io::write(&self.wake, &1_u64.to_ne_bytes()),
            Err(Errno::INTR)
        ) {}
    }

    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        self.wake();
    }
}

fn socket_path(socket: &Path) -> Result<PathBuf, String> {
    if socket.is_absolute() {
        return Ok(socket.to_owned());
    }
    let runtime = env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| format!("XDG_RUNTIME_DIR is unset for {}", socket.display()))?;
    Ok(Path::new(&runtime).join(socket))
}

fn connect_cursor(socket: &Path) -> Result<UnixStream, String> {
    let path = socket_path(socket)?;
    let open = || {
        let address = SocketAddrUnix::new(&path)?;
        let fd = socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::CLOEXEC | SocketFlags::NONBLOCK,
            None,
        )?;
        connect(&fd, &address)?;
        Ok::<_, Errno>(UnixStream::from(fd))
    };
    open().map_err(|error| format!("PipeWire cursor socket {}: {error}", path.display()))
}

fn send_cursor(cursor: &mut Option<UnixStream>, visible: bool) -> std::io::Result<()> {
    if let Some(stream) = cursor
        && let Err(error) = stream.write_all(if visible { b"1" } else { b"0" })
    {
        // Closing the socket also clears a cursor the compositor may still be showing.
        *cursor = None;
        return Err(error);
    }
    Ok(())
}

fn run(
    connection: EisConnection,
    cursor: Option<UnixStream>,
    incoming: Receiver<String>,
    shared: &Shared,
    session_id: &str,
) {
    let mut input = InputState::new(connection);
    let cursor = RefCell::new(cursor);
    let set_cursor = |visible| {
        if let Err(error) = send_cursor(&mut cursor.borrow_mut(), visible) {
            warn!("Session {session_id}: could not update the PipeWire cursor: {error}");
        }
    };
    let result = dispatch(&mut input, incoming, shared, session_id, &set_cursor);
    drop(cursor);
    if let Err(error) = result {
        warn!("Session {session_id}: {error}");
        let _ = shared.reason.set(error);
        shared.stop();
    }
    input.release_all();
    if let Err(error) = input.backend.finish() {
        warn!("Session {session_id}: {error}");
    }
    // Publish failure before closing the channel so the viewer can fetch its reason. Release
    // the mutex first because a channel close can call back into the agent.
    if shared.reason.get().is_some() {
        let channel = shared
            .channel
            .lock()
            .unwrap()
            .take()
            .and_then(|weak| weak.upgrade());
        if let Some(channel) = channel {
            channel.close();
        }
    }
}

fn dispatch(
    input: &mut InputState<EisConnection>,
    incoming: Receiver<String>,
    shared: &Shared,
    session_id: &str,
    set_cursor: &dyn Fn(bool),
) -> Result<(), String> {
    while !shared.stopping.load(Ordering::Acquire) {
        let fd = input.backend.fd();
        let mut fds = [
            PollFd::new(&fd, PollFlags::IN),
            PollFd::new(&shared.wake, PollFlags::IN),
        ];
        match poll(&mut fds, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(error) => return Err(format!("Could not poll gamescope input: {error}")),
        }
        let wake = fds[1].revents().contains(PollFlags::IN);
        if !fds[0].revents().is_empty() {
            input.backend.dispatch()?;
        }
        if wake {
            let _ = io::read(&shared.wake, &mut [0_u8; 8]);
            // Cap each batch so a full input queue cannot starve libei events.
            for _ in 0..QUEUE_SIZE {
                if shared.stopping.load(Ordering::Acquire) {
                    break;
                }
                let Ok(text) = incoming.try_recv() else {
                    break;
                };
                apply_and_log::<_, TouchInjector>(&text, input, None, set_cursor, session_id);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn sends_cursor_visibility_and_closes_on_disconnect() {
        let (stream, mut compositor) = UnixStream::pair().unwrap();
        compositor.set_nonblocking(true).unwrap();
        let mut cursor = Some(stream);
        send_cursor(&mut cursor, true).unwrap();
        send_cursor(&mut cursor, false).unwrap();
        let mut commands = [0; 2];
        compositor.read_exact(&mut commands).unwrap();
        assert_eq!(&commands, b"10");
        drop(cursor);
        assert_eq!(compositor.read(&mut commands).unwrap(), 0);
    }

    #[test]
    fn disables_cursor_after_a_socket_failure() {
        let (stream, compositor) = UnixStream::pair().unwrap();
        drop(compositor);
        let mut cursor = Some(stream);
        assert!(send_cursor(&mut cursor, true).is_err());
        assert!(cursor.is_none());
        assert!(send_cursor(&mut cursor, false).is_ok());
    }
}
