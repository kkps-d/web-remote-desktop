//! Screen capture through the ScreenCast portal: GNOME asks the person at the desktop to share a
//! monitor, then pipewiresrc streams it.

use std::os::fd::{AsRawFd, OwnedFd};
use std::path::PathBuf;
use std::pin::pin;
use std::thread;
use std::time::Duration;

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType};
use ashpd::desktop::{PersistMode, ResponseError, Session};
use ashpd::zbus::block_on;
use futures_channel::oneshot;
use futures_util::future::{self, Either};
use gstreamer as gst;
use gstreamer::prelude::*;
use tracing::{debug, warn};

use crate::stream_settings::Resolution;

// GNOME sends frames only when the screen changes; pipewiresrc resends the last one this often
// so a static screen keeps streaming.
const KEEPALIVE_MS: u32 = 100;
// How long the person at the desktop has to answer GNOME's share dialog. The viewer's offer, and
// any viewer after it, wait meanwhile.
const CONSENT_TIMEOUT: Duration = Duration::from_secs(60);

/// A ScreenCast portal session sharing one monitor, held for as long as the stream runs.
/// Dropping it ends the session and GNOME's sharing indicator.
pub struct Capture {
    session: Session<Screencast>,
    monitor: SharedMonitor,
}

struct SharedMonitor {
    // pipewiresrc connects with a duplicate of this PipeWire remote.
    remote: OwnedFd,
    node_id: u32,
    size: Option<Resolution>,
}

impl Capture {
    /// Asks the portal to share a monitor. The first time, GNOME shows a dialog on the desktop;
    /// a saved restore token skips it afterwards. Blocks until the dialog is answered, for up to
    /// `CONSENT_TIMEOUT`.
    pub fn open() -> Result<Self, String> {
        block_on(async {
            let portal = Screencast::new().await.map_err(open_error)?;
            let session = portal
                .create_session(Default::default())
                .await
                .map_err(open_error)?;
            let shared = {
                let sharing = pin!(share_monitor(&portal, &session));
                match future::select(sharing, sleep(CONSENT_TIMEOUT)).await {
                    Either::Left((shared, _)) => shared.map_err(open_error),
                    Either::Right(_) => {
                        Err("Nobody answered the screen sharing dialog in time.".to_owned())
                    }
                }
            };
            match shared {
                Ok(monitor) => Ok(Self { session, monitor }),
                // An ashpd Session doesn't close on drop. Closing also dismisses the dialog.
                Err(error) => {
                    let _ = session.close().await;
                    Err(error)
                }
            }
        })
    }

    /// The capture segment of the pipeline; its source is named "capture". GNOME's stream has a
    /// variable frame rate, so videorate caps it at the output caps' rate. The portal's format
    /// never changes; see `attach`. When the user stops sharing, pipewiresrc posts an error,
    /// which ends the session.
    pub fn description(&self) -> String {
        format!(
            "pipewiresrc fd={fd} path={node} name=capture keepalive-time={KEEPALIVE_MS} \
             on-disconnect=error ! videorate drop-only=true ! videoconvert ! videoscale",
            fd = self.monitor.remote.as_raw_fd(),
            node = self.monitor.node_id,
        )
    }

    /// Takes the element named "capture" once the pipeline is built. A settings change sends a
    /// reconfigure event upstream, and pipewiresrc answers it by reconnecting its stream, after
    /// which GNOME sends no more frames. The elements after it adapt to any change, so it never
    /// needs to renegotiate. The event is dropped on the next element's sink pad, since
    /// arriving on pipewiresrc's pad already flags it for renegotiation.
    pub fn attach(&self, capture: &gst::Element) {
        let Some(next_sink) = capture.static_pad("src").and_then(|src| src.peer()) else {
            return;
        };
        next_sink.add_probe(gst::PadProbeType::EVENT_UPSTREAM, |_, info| {
            match info.event() {
                Some(event) if event.type_() == gst::EventType::Reconfigure => {
                    gst::PadProbeReturn::Drop
                }
                _ => gst::PadProbeReturn::Ok,
            }
        });
    }

    /// The shared monitor's size, as the portal reports it.
    pub fn size(&self) -> Option<Resolution> {
        self.monitor.size
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Err(error) = block_on(self.session.close()) {
            debug!("Could not close the screen cast session: {error}");
        }
    }
}

/// Completes after `duration` on any executor. A thread does the waiting, since zbus's
/// `block_on` offers no timer of its own.
fn sleep(duration: Duration) -> oneshot::Receiver<()> {
    let (elapsed, receiver) = oneshot::channel();
    thread::spawn(move || {
        thread::sleep(duration);
        let _ = elapsed.send(());
    });
    receiver
}

fn open_error(error: ashpd::Error) -> String {
    match error {
        ashpd::Error::Response(ResponseError::Cancelled) => "Screen sharing was declined.".into(),
        error => format!("Could not start screen sharing: {error}"),
    }
}

async fn share_monitor(
    portal: &Screencast,
    session: &Session<Screencast>,
) -> ashpd::Result<SharedMonitor> {
    let saved_token = restore_token_path().and_then(|path| std::fs::read_to_string(path).ok());
    portal
        .select_sources(
            session,
            SelectSourcesOptions::default()
                .set_sources(SourceType::MONITOR)
                .set_multiple(false)
                .set_cursor_mode(CursorMode::EMBEDDED)
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(saved_token.as_deref().map(str::trim)),
        )
        .await?;
    let streams = portal
        .start(session, None, Default::default())
        .await?
        .response()?;
    if let Some(token) = streams.restore_token() {
        save_restore_token(token);
    }
    let stream = streams
        .streams()
        .first()
        .ok_or(ashpd::Error::Response(ResponseError::Other))?;
    let size = stream
        .size()
        .map(|(width, height)| Resolution { width, height });
    let node_id = stream.pipe_wire_node_id();
    let remote = portal
        .open_pipe_wire_remote(session, Default::default())
        .await?;
    Ok(SharedMonitor {
        remote,
        node_id,
        size,
    })
}

// The token lets later sessions, also after a restart, share the same monitor without the
// dialog. Each session returns a new one.
fn restore_token_path() -> Option<PathBuf> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/state"))
        })?;
    Some(state.join("web-remote-desktop/screencast-restore-token"))
}

fn save_restore_token(token: &str) {
    let Some(path) = restore_token_path() else {
        return;
    };
    let saved = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&path, token));
    if let Err(error) = saved {
        warn!(
            "Could not save the screen cast restore token to {}: {error}",
            path.display()
        );
    }
}
