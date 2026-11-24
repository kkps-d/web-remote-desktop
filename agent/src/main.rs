mod config;
mod encode_timing;
mod encoders;
mod http;
mod input;
mod named_audio;
mod pipeline;
mod platform;
mod sdp;
mod stream_session;
mod stream_settings;
mod viewer_sessions;

use std::net::Ipv4Addr;

use tokio::net::TcpListener;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::encoders::EncoderInventory;
use crate::viewer_sessions::ViewerSessions;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = match Config::from_args(std::env::args().skip(1)) {
        Ok(config) => config,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    if let Some(orchprox) = &config.orchprox {
        warn!(
            "Connecting to the orchprox at {} isn't implemented yet; ignoring it.",
            orchprox.url
        );
    }

    if let Err(error) = gstreamer::init() {
        eprintln!("Could not initialize GStreamer: {error}");
        std::process::exit(1);
    }
    platform::use_physical_pixels();
    let sessions = match EncoderInventory::discover() {
        Ok(encoders) => ViewerSessions::new(encoders, config.stream, config.capture),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let shutdown = async {
        #[cfg(target_os = "linux")]
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = terminate.recv() => {}
                _ = tokio::signal::ctrl_c() => {}
            }
            return;
        }
        let _ = tokio::signal::ctrl_c().await;
    };
    if let Some(direct) = &config.direct {
        let port = direct.port;
        let address = if direct.loopback_only {
            Some(Ipv4Addr::LOCALHOST)
        } else {
            direct.bind_address
        };
        let bind = match address {
            Some(address) => TcpListener::bind((address, port)).await,
            None => http::listen_any_ip(port),
        };
        let listener = match bind {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("Could not listen on port {port}: {error}");
                std::process::exit(1);
            }
        };
        info!("Listening on port {port}");
        axum::serve(listener, http::router(sessions.clone()))
            .with_graceful_shutdown(shutdown)
            .await
            .expect("the HTTP server failed");
    } else {
        info!("Direct connections are off");
        shutdown.await;
    }
    sessions.shutdown();
}
