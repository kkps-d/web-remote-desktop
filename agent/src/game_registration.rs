//! Optional outbound registration for an agent launched by the game session service.

use std::env;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio_tungstenite::connect_async_with_config;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Request;
use tokio_tungstenite::tungstenite::http::header::{AUTHORIZATION, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::{Message, WebSocketConfig};
use tracing::{info, warn};

use crate::game_signaling;
use crate::viewer_sessions::ViewerSessions;

pub fn from_env() -> Result<Option<Request>, &'static str> {
    match (
        env::var("GAME_STREAM_AGENT_URL").ok(),
        env::var("GAME_STREAM_LAUNCH_TOKEN").ok(),
    ) {
        (None, None) => Ok(None),
        (Some(url), Some(token)) => registration_request(&url, &token).map(Some),
        _ => Err("Set GAME_STREAM_AGENT_URL and GAME_STREAM_LAUNCH_TOKEN together."),
    }
}

fn registration_request(url: &str, token: &str) -> Result<Request, &'static str> {
    const URL_ERROR: &str =
        "GAME_STREAM_AGENT_URL must point to the agent endpoint on a private IPv4 address.";
    let mut request = url.into_client_request().map_err(|_| URL_ERROR)?;
    let uri = request.uri();
    let private_host = uri
        .host()
        .and_then(|host| host.parse::<Ipv4Addr>().ok())
        .is_some_and(|ip| ip.is_private() || ip.is_loopback());
    if uri.scheme_str() != Some("ws")
        || !private_host
        || !uri.path().starts_with("/api/agents/connect/")
        || uri.query().is_some()
        || uri
            .authority()
            .is_some_and(|host| host.as_str().contains('@'))
    {
        return Err(URL_ERROR);
    }
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("Invalid GAME_STREAM_LAUNCH_TOKEN.");
    }
    let mut bearer = HeaderValue::from_str(&format!("Bearer {token}"))
        .map_err(|_| "Invalid GAME_STREAM_LAUNCH_TOKEN.")?;
    bearer.set_sensitive(true);
    request.headers_mut().insert(AUTHORIZATION, bearer);
    Ok(request)
}

pub async fn run(request: Request, sessions: Arc<ViewerSessions>) {
    let config = WebSocketConfig::default()
        .max_message_size(Some(256 * 1024))
        .max_frame_size(Some(256 * 1024));
    let connection = timeout(
        Duration::from_secs(5),
        connect_async_with_config(request, Some(config), false),
    )
    .await;
    let Ok(Ok((mut socket, _))) = connection else {
        warn!(
            "Could not register with the game session service. Stop and relaunch the game to retry."
        );
        return;
    };
    info!("Registered with the game session service.");
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(request) => {
                let Some(response) = game_signaling::respond(sessions.clone(), &request).await
                else {
                    warn!("Invalid game signaling request.");
                    break;
                };
                if socket.send(Message::Text(response.into())).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => {
                let _ = socket.flush().await;
                break;
            }
            _ => {}
        }
    }
    // An independent direct desktop session can still use the agent.
    let _ = tokio::task::spawn_blocking(move || sessions.stop_relayed()).await;
    info!("Game session service connection closed.");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_uses_a_sensitive_bearer_on_the_private_endpoint() {
        let token = "a".repeat(64);
        let request =
            registration_request("ws://172.30.42.1:3100/api/agents/connect/id", &token).unwrap();
        let header = &request.headers()[AUTHORIZATION];
        assert_eq!(header.to_str().unwrap(), format!("Bearer {token}"));
        assert!(header.is_sensitive());
    }

    #[test]
    fn rejects_public_hosts_query_strings_and_invalid_tokens() {
        let token = "a".repeat(64);
        for url in [
            "ws://example.com/api/agents/connect/id",
            "ws://172.30.42.1/api/agents/connect/id?token=value",
            "ws://user@172.30.42.1/api/agents/connect/id",
        ] {
            assert!(registration_request(url, &token).is_err());
        }
        assert!(registration_request("ws://172.30.42.1/api/agents/connect/id", "bad").is_err());
    }
}
