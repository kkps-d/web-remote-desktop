//! Holds the single active viewer session. A new viewer replaces the current one.

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tracing::info;

use crate::encode_timing::EncodingStats;
use crate::encoders::{EncoderCatalog, EncoderOption};
use crate::sdp;
use crate::session::{StartError, StreamSession, UpdateError};
use crate::settings::{Resolution, StreamSettings, StreamSettingsUpdate};

/// What the client sees about a session's stream.
#[derive(Serialize)]
pub struct StreamInfo {
    source: Resolution,
    settings: StreamSettings,
    encoders: Vec<EncoderOption>,
}

pub struct ViewerSessions {
    encoders: EncoderCatalog,
    // Serializes setup so only one pipeline captures the desktop at a time. Setup blocks for
    // seconds, so it has its own lock and stopping a session never waits for it.
    setup: Mutex<()>,
    current: Mutex<Option<Arc<StreamSession>>>,
}

impl ViewerSessions {
    pub fn new(encoders: EncoderCatalog) -> Arc<Self> {
        Arc::new(Self {
            encoders,
            setup: Mutex::new(()),
            current: Mutex::new(None),
        })
    }

    /// Starts a session for the offer, replacing the current one. Returns its id and SDP answer.
    pub fn start(self: &Arc<Self>, offer_sdp: &str) -> Result<(String, String), StartError> {
        // Reject an unusable offer before it disconnects the current viewer.
        let codec = sdp::find_h264_codec(offer_sdp)?;

        let _setup = self.setup.lock().unwrap();
        if let Some(previous) = self.current.lock().unwrap().take() {
            info!("Replacing session {} with a new viewer", previous.id());
            previous.stop();
        }

        let sessions = Arc::downgrade(self);
        let on_ended = move |id: &str| {
            if let Some(sessions) = sessions.upgrade() {
                sessions.stop(id);
            }
        };
        let (session, answer) = StreamSession::start(offer_sdp, &codec, &self.encoders, on_ended)?;

        let mut current = self.current.lock().unwrap();
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
        let mut current = self.current.lock().unwrap();
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
        let updated = session.update(update, &self.encoders);
        Some(updated.map(|()| self.info_of(&session)))
    }

    fn find(&self, id: &str) -> Option<Arc<StreamSession>> {
        let current = self.current.lock().unwrap();
        current
            .as_ref()
            .filter(|session| session.id() == id)
            .cloned()
    }

    fn info_of(&self, session: &StreamSession) -> StreamInfo {
        StreamInfo {
            source: session.source_resolution(),
            settings: session.settings(),
            encoders: self.encoders.options().to_vec(),
        }
    }

    /// Stops the current session, waiting for any setup in progress.
    pub fn shutdown(&self) {
        let _setup = self.setup.lock().unwrap();
        if let Some(session) = self.current.lock().unwrap().take() {
            session.stop();
        }
    }
}
