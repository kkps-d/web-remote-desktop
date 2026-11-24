//! Viewer requests that arrive on the game's authenticated outbound connection.

use std::sync::Arc;

use axum::Json;
use axum::body::to_bytes;
use axum::extract::{Path, State};
use axum::http::{StatusCode, header::LOCATION};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::http;
use crate::viewer_sessions::ViewerSessions;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    request_id: String,
    #[serde(flatten)]
    operation: Operation,
}

#[derive(Deserialize)]
#[serde(
    tag = "operation",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Operation {
    Offer { sdp: String, turn_url: String },
    Info { session_id: String },
    Stats { session_id: String },
    Update { session_id: String, settings: Value },
    Stop { session_id: String },
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    request_id: String,
    status: u16,
    body: String,
    session_id: Option<String>,
}

pub async fn respond(sessions: Arc<ViewerSessions>, text: &str) -> Option<String> {
    let Request {
        request_id,
        operation,
    } = serde_json::from_str(text).ok()?;
    let response = match operation {
        Operation::Offer { sdp, turn_url } => {
            http::start_stream(sessions, sdp, Some(turn_url)).await
        }
        Operation::Info { session_id } => http::info(State(sessions), Path(session_id)).await,
        Operation::Stats { session_id } => http::stats(State(sessions), Path(session_id)).await,
        Operation::Update {
            session_id,
            settings,
        } => match serde_json::from_value(settings) {
            Ok(settings) => {
                http::update(State(sessions), Path(session_id), Ok(Json(settings))).await
            }
            Err(_) => StatusCode::BAD_REQUEST.into_response(),
        },
        Operation::Stop { session_id } => http::stop(State(sessions), Path(session_id))
            .await
            .into_response(),
    };
    let (parts, body) = response.into_parts();
    let session_id = parts
        .headers
        .get(LOCATION)
        .and_then(|location| location.to_str().ok()?.strip_prefix("/sessions/"))
        .map(str::to_owned);
    let body = to_bytes(body, 64 * 1024).await.ok()?;
    serde_json::to_string(&Reply {
        request_id,
        status: parts.status.as_u16(),
        body: String::from_utf8(body.to_vec()).ok()?,
        session_id,
    })
    .ok()
}
