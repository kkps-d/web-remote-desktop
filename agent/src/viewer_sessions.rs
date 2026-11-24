//! Holds the single active viewer session. A new viewer replaces the current one.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::Serialize;
use tracing::info;

use crate::config::CaptureConfig;
use crate::encode_timing::EncodingStats;
use crate::encoders::{EncoderInventory, EncoderOption};
use crate::sdp;
use crate::stream_session::{StartError, StreamSession, UpdateError};
use crate::stream_settings::{Resolution, StreamConfig, StreamSettings, StreamSettingsUpdate};

/// What the client sees about a session's stream.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamInfo {
    source: Resolution,
    settings: StreamSettings,
    encoders: Vec<EncoderOption>,
    limits: StreamLimits,
    /// Why the stream has no audio, or None if it has.
    audio_unavailable_reason: Option<String>,
    input_unavailable_reason: Option<String>,
    touch_supported: bool,
}

/// The most this agent allows. `resolution` is the source, or scaled down to fit the agent's
/// maximum resolution.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamLimits {
    resolution: Resolution,
    framerate: i32,
    bitrate_kbps: i32,
}

pub struct ViewerSessions {
    encoders: EncoderInventory,
    config: StreamConfig,
    capture: Option<CaptureConfig>,
    // Serializes setup so only one pipeline captures the desktop at a time. Setup blocks for
    // seconds, so it has its own lock and stopping a session never waits for it.
    setup: Mutex<()>,
    current: Mutex<Option<Arc<StreamSession>>>,
}

impl ViewerSessions {
    pub fn new(
        encoders: EncoderInventory,
        config: StreamConfig,
        capture: Option<CaptureConfig>,
    ) -> Arc<Self> {
        Arc::new(Self {
            encoders,
            config,
            capture,
            setup: Mutex::new(()),
            current: Mutex::new(None),
        })
    }

    /// Starts a session for the offer, replacing the current one. Returns its id and SDP answer.
    pub fn start(
        self: &Arc<Self>,
        offer_sdp: &str,
        turn_url: Option<&str>,
    ) -> Result<(String, String), StartError> {
        // Reject an unusable offer before it disconnects the current viewer.
        let codec = sdp::find_h264_codec(offer_sdp)?;

        let _setup = lock(&self.setup);
        if let Some(previous) = lock(&self.current).take() {
            info!("Replacing session {} with a new viewer", previous.id());
            previous.stop();
        }

        let sessions = Arc::downgrade(self);
        let on_ended = move |id: &str| {
            if let Some(sessions) = sessions.upgrade() {
                sessions.stop(id);
            }
        };
        let (session, answer) = StreamSession::start(
            offer_sdp,
            &codec,
            &self.encoders,
            &self.config,
            self.capture.as_ref(),
            turn_url,
            on_ended,
        )?;

        let mut current = lock(&self.current);
        // stop() ignores a session that isn't current yet, so catch one that already ended.
        if let Some(reason) = session.end_reason() {
            session.stop();
            return Err(StartError::Failed(reason.into()));
        }
        info!("Session {} started", session.id());
        let id = session.id().to_owned();
        *current = Some(session);
        Ok((id, answer))
    }

    /// Stops the session if it is still the current one. Returns false if it isn't.
    pub fn stop(&self, id: &str) -> bool {
        let mut current = lock(&self.current);
        if current.as_ref().is_none_or(|session| session.id() != id) {
            return false;
        }
        info!("Session {id} stopped");
        if let Some(session) = current.take() {
            session.stop();
        }
        true
    }

    /// The session's stream info, or None if it isn't the current session.
    pub fn info(&self, id: &str) -> Option<StreamInfo> {
        self.find(id).map(|session| self.info_of(&session))
    }

    pub fn stats(&self, id: &str) -> Option<EncodingStats> {
        self.find(id).map(|session| session.encoding_stats())
    }

    /// Changes the session's stream settings. Returns None if it isn't the current session.
    pub fn update(
        &self,
        id: &str,
        update: &StreamSettingsUpdate,
    ) -> Option<Result<StreamInfo, UpdateError>> {
        // The lock isn't held while updating: a failed encoder switch ends the session, and
        // ending takes the lock to stop it. The Arc keeps the session valid meanwhile.
        let session = self.find(id)?;
        let updated = session.update(update, &self.config);
        Some(updated.map(|()| self.info_of(&session)))
    }

    fn find(&self, id: &str) -> Option<Arc<StreamSession>> {
        let current = lock(&self.current);
        current
            .as_ref()
            .filter(|session| session.id() == id)
            .cloned()
    }

    fn info_of(&self, session: &StreamSession) -> StreamInfo {
        let source = session.source_resolution();
        StreamInfo {
            source,
            settings: session.settings(),
            encoders: session.encoder_options().to_vec(),
            limits: StreamLimits {
                resolution: self.config.largest_size(source),
                framerate: self.config.max_fps,
                bitrate_kbps: self.config.max_bitrate_kbps,
            },
            audio_unavailable_reason: session.audio_unavailable().map(str::to_owned),
            input_unavailable_reason: session.input_unavailable().map(str::to_owned),
            touch_supported: session.touch_supported(),
        }
    }

    pub fn stop_relayed(&self) {
        let mut current = lock(&self.current);
        if current.as_ref().is_some_and(|session| session.is_relayed())
            && let Some(session) = current.take()
        {
            session.stop();
        }
    }

    /// Stops the current session, waiting for any setup in progress.
    pub fn shutdown(&self) {
        let _setup = lock(&self.setup);
        if let Some(session) = lock(&self.current).take() {
            session.stop();
        }
    }
}

// A panic while a lock is held, e.g. during setup, poisons it. What the locks guard stays valid
// (a unit, or an Option that is only ever replaced whole), so later viewers take them anyway
// instead of failing until the agent restarts.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use std::panic;

    use super::*;

    #[test]
    fn takes_a_lock_that_a_panic_poisoned() {
        let setup = Mutex::new(Some(1));
        let _ = panic::catch_unwind(|| {
            let _held = setup.lock().unwrap();
            panic!("setup failed");
        });
        assert!(setup.is_poisoned());
        assert_eq!(*lock(&setup), Some(1));
    }
}
