//! Direct connections: the HTTP signaling routes and the listener they're served on.

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::header::{CONTENT_TYPE, LOCATION};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use socket2::{Domain, Protocol, Socket, Type};
use tower_http::cors::{AllowHeaders, AllowMethods, Any, CorsLayer};
use tracing::{error, warn};

use crate::stream_session::{StartError, UpdateError};
use crate::stream_settings::StreamSettingsUpdate;
use crate::viewer_sessions::ViewerSessions;

/// WHEP-style signaling: POST an SDP offer, get the SDP answer and a session URL back.
pub fn router(sessions: Arc<ViewerSessions>) -> Router {
    // The web client is hosted on a different origin, and needs the Location header to end its session.
    // Requested headers and methods are echoed back; a `*` wouldn't cover Authorization.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
        .expose_headers([LOCATION]);
    Router::new()
        .route("/offer", post(offer))
        .route("/sessions/{id}", get(info).patch(update).delete(stop))
        .route("/sessions/{id}/stats", get(stats))
        .layer(cors)
        .with_state(sessions)
}

async fn offer(
    State(sessions): State<Arc<ViewerSessions>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let content_type = headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok());
    if !content_type.is_some_and(|value| value.starts_with("application/sdp")) {
        let message = "Expected Content-Type: application/sdp";
        return (StatusCode::UNSUPPORTED_MEDIA_TYPE, message).into_response();
    }

    start_stream(sessions, String::from_utf8_lossy(&body).into_owned(), None).await
}

pub(crate) async fn start_stream(
    sessions: Arc<ViewerSessions>,
    offer: String,
    turn_url: Option<String>,
) -> Response {
    // Session setup blocks on GStreamer for up to a few seconds; keep it off the async workers.
    let started = tokio::task::spawn_blocking(move || sessions.start(&offer, turn_url.as_deref()))
        .await
        .unwrap_or_else(|panic| Err(StartError::Failed(panic.to_string())));
    match started {
        Ok((id, answer)) => {
            let headers = [
                (LOCATION, format!("/sessions/{id}")),
                (CONTENT_TYPE, "application/sdp".into()),
            ];
            (StatusCode::CREATED, headers, answer).into_response()
        }
        Err(StartError::InvalidOffer(message)) => {
            (StatusCode::BAD_REQUEST, message).into_response()
        }
        Err(StartError::Failed(message)) => {
            error!("Failed to start a session: {message}");
            let message = format!("Could not start the stream: {message}");
            (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
        }
    }
}

// The source resolution, current stream settings (resolution, frame rate, bitrate, encoder)
// and the encoders this computer can use.
pub(crate) async fn info(
    State(sessions): State<Arc<ViewerSessions>>,
    Path(id): Path<String>,
) -> Response {
    match sessions.info(&id) {
        Some(info) => Json(info).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub(crate) async fn stats(
    State(sessions): State<Arc<ViewerSessions>>,
    Path(id): Path<String>,
) -> Response {
    match sessions.stats(&id) {
        Some(stats) => Json(stats).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// Changes stream settings while streaming. Omitted fields keep their current value.
pub(crate) async fn update(
    State(sessions): State<Arc<ViewerSessions>>,
    Path(id): Path<String>,
    update: Result<Json<StreamSettingsUpdate>, JsonRejection>,
) -> Response {
    let update = match update {
        Ok(Json(update)) => update,
        Err(rejection @ JsonRejection::MissingJsonContentType(_)) => {
            return (StatusCode::UNSUPPORTED_MEDIA_TYPE, rejection.body_text()).into_response();
        }
        Err(rejection) => return (StatusCode::BAD_REQUEST, rejection.body_text()).into_response(),
    };

    // An encoder switch blocks until the next frame.
    let updated = tokio::task::spawn_blocking(move || sessions.update(&id, &update))
        .await
        .unwrap_or_else(|panic| Some(Err(UpdateError::Failed(panic.to_string()))));
    match updated {
        Some(Ok(info)) => Json(info).into_response(),
        None | Some(Err(UpdateError::Stopped)) => StatusCode::NOT_FOUND.into_response(),
        Some(Err(UpdateError::InvalidSettings(message))) => {
            (StatusCode::BAD_REQUEST, message).into_response()
        }
        // An encoder that fails to start ends the session; tell the client why.
        Some(Err(UpdateError::Failed(message))) => {
            (StatusCode::INTERNAL_SERVER_ERROR, message).into_response()
        }
    }
}

pub(crate) async fn stop(
    State(sessions): State<Arc<ViewerSessions>>,
    Path(id): Path<String>,
) -> StatusCode {
    // Stopping waits for the pipeline to shut down.
    let stopped = tokio::task::spawn_blocking(move || sessions.stop(&id)).await;
    if stopped.unwrap_or(false) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// Listens on every IPv4 and IPv6 address, like Kestrel's `ListenAnyIP`: a machine without IPv6
/// gets IPv4 only, but a port in use is an error either way.
pub fn listen_any_ip(port: u16) -> io::Result<tokio::net::TcpListener> {
    match listen(Domain::IPV6, port) {
        Err(error) if error.kind() != io::ErrorKind::AddrInUse => {
            warn!("Listening on IPv4 only, because IPv6 is unavailable: {error}");
            listen(Domain::IPV4, port)
        }
        result => result,
    }
}

// SO_REUSEADDR stays off on Windows, where it would let another process take over the port.
// Elsewhere it only lets a restarted agent listen while the last run's connections linger in
// TIME-WAIT.
fn listen(domain: Domain, port: u16) -> io::Result<tokio::net::TcpListener> {
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
    #[cfg(not(windows))]
    socket.set_reuse_address(true)?;
    let address = if domain == Domain::IPV6 {
        socket.set_only_v6(false)?;
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))
    } else {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, port))
    };
    socket.bind(&address.into())?;
    socket.listen(1024)?;
    socket.set_nonblocking(true)?;
    tokio::net::TcpListener::from_std(socket.into())
}
