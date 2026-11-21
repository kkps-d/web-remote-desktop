mod config;
mod encode_timing;
mod encoders;
mod http;
mod input;
mod pipeline;
mod platform;
mod sdp;
mod stream_session;
mod stream_settings;
mod viewer_sessions;

use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use crate::config::Config;
use crate::encoders::EncoderCatalog;
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
    // Detect encoders at startup, so the log shows them and the first viewer doesn't wait.
    let sessions = match EncoderCatalog::detect() {
        Ok(encoders) => ViewerSessions::new(encoders, config.stream.clone()),
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    };

    let shutdown = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    if let Some(direct) = &config.direct {
        let port = direct.port;
        let listener = match http::listen_any_ip(port) {
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
