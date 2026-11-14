mod encode_timing;
mod encoders;
// Used from a later phase on; each allow goes once its module is fully used.
#[allow(dead_code)]
mod input;
mod platform;
#[allow(dead_code)]
mod sdp;
mod session;
mod sessions;
mod settings;

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
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::encoders::EncoderCatalog;
use crate::session::{StartError, UpdateError};
use crate::sessions::ViewerSessions;
use crate::settings::StreamSettingsUpdate;

const DEFAULT_PORT: u16 = 8080;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let port = match parse_port(std::env::args().skip(1)) {
        Ok(port) => port,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    if let Err(error) = gstreamer::init() {
        eprintln!("Could not initialize GStreamer: {error}");
        std::process::exit(1);
    }
    // Detect encoders at startup, so the log shows them and the first viewer doesn't wait.
    let sessions = match EncoderCatalog::detect() {
        Ok(encoders) => ViewerSessions::new(encoders),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    // The web client is hosted on a different origin, and needs the Location header to end its session.
    // Requested headers and methods are echoed back like ASP.NET's AllowAnyHeader and
    // AllowAnyMethod; a `*` wouldn't cover Authorization.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
        .expose_headers([LOCATION]);
    // WHEP-style signaling: POST an SDP offer, get the SDP answer and a session URL back.
    let app = Router::new()
        .route("/offer", post(offer))
        .route("/sessions/{id}", get(info).patch(update).delete(stop))
        .route("/sessions/{id}/stats", get(stats))
        .layer(cors)
        .with_state(sessions.clone());

    let listener = match listen_any_ip(port) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("Could not listen on port {port}: {error}");
            std::process::exit(1);
        }
    };
    info!("Listening on port {port}");

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .expect("the HTTP server failed");
    sessions.shutdown();
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

    let offer = String::from_utf8_lossy(&body).into_owned();
    // Session setup blocks on GStreamer for up to a few seconds; keep it off the async workers.
    let started = tokio::task::spawn_blocking(move || sessions.start(&offer))
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
async fn info(State(sessions): State<Arc<ViewerSessions>>, Path(id): Path<String>) -> Response {
    match sessions.info(&id) {
        Some(info) => Json(info).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn stats(State(sessions): State<Arc<ViewerSessions>>, Path(id): Path<String>) -> Response {
    match sessions.stats(&id) {
        Some(stats) => Json(stats).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

// Changes stream settings while streaming. Omitted fields keep their current value.
async fn update(
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

async fn stop(State(sessions): State<Arc<ViewerSessions>>, Path(id): Path<String>) -> StatusCode {
    // Stopping waits for the pipeline to shut down.
    let stopped = tokio::task::spawn_blocking(move || sessions.stop(&id)).await;
    if stopped.unwrap_or(false) {
        StatusCode::NO_CONTENT
    } else {
        StatusCode::NOT_FOUND
    }
}

/// Reads `--port N` or `--port=N`. The name is case-insensitive, so the C# agent's `--Port` works too.
fn parse_port(mut args: impl Iterator<Item = String>) -> Result<u16, String> {
    let mut port = DEFAULT_PORT;
    while let Some(arg) = args.next() {
        let value = match arg.split_once('=') {
            Some((name, value)) if name.eq_ignore_ascii_case("--port") => value.to_owned(),
            None if arg.eq_ignore_ascii_case("--port") => args
                .next()
                .ok_or("--port needs a value, e.g. --port 9000")?,
            _ => {
                return Err(format!(
                    "Unknown argument {arg}. Usage: agent [--port 8080]"
                ));
            }
        };
        port = value.parse().map_err(|_| format!("Invalid port {value}"))?;
    }
    Ok(port)
}

/// Listens on every IPv4 and IPv6 address, like Kestrel's `ListenAnyIP`: a machine without IPv6
/// gets IPv4 only, but a port in use is an error either way.
fn listen_any_ip(port: u16) -> io::Result<tokio::net::TcpListener> {
    match listen(Domain::IPV6, port) {
        Err(error) if error.kind() != io::ErrorKind::AddrInUse => {
            warn!("Listening on IPv4 only, because IPv6 is unavailable: {error}");
            listen(Domain::IPV4, port)
        }
        result => result,
    }
}

// SO_REUSEADDR stays off: on Windows it would let another process take over the port.
fn listen(domain: Domain, port: u16) -> io::Result<tokio::net::TcpListener> {
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))?;
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
