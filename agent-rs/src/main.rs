// Used by the streaming session from the next phase on; each allow goes once its module is used.
#[allow(dead_code)]
mod input;
#[allow(dead_code)]
mod platform;
#[allow(dead_code)]
mod sdp;
#[allow(dead_code)]
mod settings;

use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};

use axum::Router;
use axum::http::header::LOCATION;
use socket2::{Domain, Protocol, Socket, Type};
use tower_http::cors::{AllowHeaders, AllowMethods, Any, CorsLayer};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

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

    // The web client is hosted on a different origin, and needs the Location header to end its session.
    // Requested headers and methods are echoed back like ASP.NET's AllowAnyHeader and
    // AllowAnyMethod; a `*` wouldn't cover Authorization.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(AllowMethods::mirror_request())
        .allow_headers(AllowHeaders::mirror_request())
        .expose_headers([LOCATION]);
    let app = Router::new().layer(cors);

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
