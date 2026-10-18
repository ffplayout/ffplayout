use std::time::Duration;

use axum::{
    Json, Router,
    extract::{Query, State},
    response::IntoResponse,
    routing::get,
};
#[cfg(test)]
use moq_mux::{catalog, container::ts};
use serde::Deserialize;

use crate::{api::state::AppState, db::handles, utils::errors::ServiceError};

#[cfg(test)]
use super::importer::configure_catalog;
use super::relay::RUNTIME;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/moq/config", get(public_bootstrap))
        .route("/api/moq/config", get(private_bootstrap))
        .route("/moq/relay", get(relay))
}

#[derive(Deserialize)]
struct ChannelQuery {
    channel: i32,
    ticket: Option<String>,
}

async fn public_bootstrap(
    State(state): State<AppState>,
    Query(query): Query<ChannelQuery>,
) -> Result<impl IntoResponse, ServiceError> {
    if !super::access::allowed(&state.pool, query.channel, None).await {
        return Err(ServiceError::NotFound(
            "Public MoQ player is disabled for this channel".into(),
        ));
    }

    bootstrap(query.channel, None)
}

async fn private_bootstrap(
    State(state): State<AppState>,
    Query(query): Query<ChannelQuery>,
    user: crate::api::routes::AuthUser,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, ServiceError> {
    user.ensure_channel_or_admin(query.channel)?;
    if handles::moq_access(&state.pool, query.channel)
        .await?
        .is_none()
    {
        return Err(ServiceError::NotFound(
            "MoQ output is disabled for this channel".into(),
        ));
    }
    let bearer = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| ServiceError::Unauthorized("Missing bearer token".into()))?;

    let response = bootstrap(query.channel, Some(bearer.to_string()))?;
    if let Some(manager) = state.controller.read().await.get(query.channel) {
        manager.moq_demand.request_start();
    }

    Ok(response)
}

fn bootstrap(channel: i32, bearer: Option<String>) -> Result<impl IntoResponse, ServiceError> {
    let runtime = RUNTIME
        .get()
        .ok_or_else(|| ServiceError::ServiceUnavailable("MoQ relay is unavailable".into()))?;
    let ticket = bearer
        .map(|bearer| runtime.tickets.issue(channel, bearer))
        .transpose()?;

    Ok((
        [("Cache-Control", "no-store")],
        Json(serde_json::json!({
            "broadcast": format!("channel-{channel}.hang"), "relay_url": runtime.public_url,
            "relay_port": runtime.port, "delay_ms": 250, "ticket": ticket
        })),
    ))
}

async fn relay(
    State(state): State<AppState>,
    Query(query): Query<ChannelQuery>,
    ws: axum::extract::ws::WebSocketUpgrade,
    headers: axum::http::HeaderMap,
) -> Result<axum::response::Response, ServiceError> {
    // The relay itself authenticates tickets and scopes subscriptions for every
    // transport. The proxy additionally refuses anonymous private-channel access.
    if query.ticket.is_none() && !super::access::allowed(&state.pool, query.channel, None).await {
        return Err(ServiceError::NotFound(
            "Public MoQ player is disabled for this channel".into(),
        ));
    }
    let runtime = RUNTIME
        .get()
        .ok_or_else(|| ServiceError::ServiceUnavailable("MoQ relay is unavailable".into()))?;
    let mut upstream = url::Url::parse(&format!("ws://127.0.0.1:{}/channels", runtime.port))
        .map_err(|_| ServiceError::InternalServerError)?;
    upstream
        .query_pairs_mut()
        .append_pair("channel", &query.channel.to_string());
    if let Some(ticket) = query.ticket {
        upstream.query_pairs_mut().append_pair("ticket", &ticket);
    }

    websocket(ws, headers, upstream.into()).await
}

pub(super) async fn websocket(
    ws: axum::extract::ws::WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    upstream: String,
) -> Result<axum::response::Response, ServiceError> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = upstream
        .into_client_request()
        .map_err(|_| ServiceError::InternalServerError)?;

    if let Some(protocols) = headers.get("sec-websocket-protocol") {
        request
            .headers_mut()
            .insert("sec-websocket-protocol", protocols.clone());
    }

    let (mut relay, response) = tokio::time::timeout(
        Duration::from_secs(5),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .map_err(|_| ServiceError::ServiceUnavailable("MoQ relay timed out".into()))?
    .map_err(|_| ServiceError::ServiceUnavailable("MoQ relay unavailable".into()))?;
    let protocol = response
        .headers()
        .get("sec-websocket-protocol")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let ws = if let Some(protocol) = protocol {
        ws.protocols([protocol])
    } else {
        ws
    };

    Ok(ws
        .on_upgrade(move |mut browser| async move {
            use axum::extract::ws::Message as Browser;
            use futures_util::{SinkExt, StreamExt};
            use tokio_tungstenite::tungstenite::Message as Relay;

            loop {
                tokio::select! {
                    message = browser.recv() => {
                        let Some(Ok(message)) = message else { break; };
                        let message = match message {
                            Browser::Binary(data) => Relay::Binary(data),
                            Browser::Text(text) => Relay::Text(text.to_string().into()),
                            Browser::Ping(data) => Relay::Ping(data),
                            Browser::Pong(data) => Relay::Pong(data),
                            Browser::Close(_) => break,
                        };

                        if relay.send(message).await.is_err() { break; }
                    }
                    message = relay.next() => {
                        let Some(Ok(message)) = message else { break; };
                        let message = match message {
                            Relay::Binary(data) => Browser::Binary(data),
                            Relay::Text(text) => Browser::Text(text.to_string().into()),
                            Relay::Ping(data) => Browser::Ping(data),
                            Relay::Pong(data) => Browser::Pong(data),
                            Relay::Close(_) => break,
                            Relay::Frame(_) => continue,
                        };

                        if browser.send(message).await.is_err() { break; }
                    }
                }
            }

            let _ = relay.close(None).await;
            let _ = browser.close().await;
        })
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::file_access::FileAccessState,
        player::controller::ChannelController,
        sse::{SseAuthState, broadcast::Broadcaster},
        utils::system::SystemStat,
    };
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use std::sync::Arc;
    use tokio::sync::{Mutex, RwLock};
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;

    #[test]
    fn decoder_hint_preserves_media_configuration() {
        let original = serde_json::json!({
            "video": {"renditions": {"0.avc3": {
                "codec": "avc3.64001f", "codedWidth": 960, "codedHeight": 540,
                "framerate": 25, "jitter": 68
            }}},
            "audio": {"renditions": {"1.aac": {
                "codec": "mp4a.40.2", "sampleRate": 48000, "numberOfChannels": 2
            }}}
        });
        let mut catalog: catalog::hang::Catalog<ts::Ext> =
            serde_json::from_value(original).unwrap();
        let before = serde_json::to_value(&catalog).unwrap();
        configure_catalog(&mut catalog);
        configure_catalog(&mut catalog);
        let mut after = serde_json::to_value(catalog).unwrap();
        assert_eq!(
            after["video"]["renditions"]["0.avc3"]
                .as_object_mut()
                .unwrap()
                .remove("optimizeForLatency"),
            Some(serde_json::json!(true))
        );
        assert_eq!(after, before);
    }

    #[tokio::test]
    async fn public_access_is_opt_in_and_private_bootstrap_requires_auth() {
        let (manager, pool) = crate::test_support::channel_manager().await;
        let system = SystemStat::new();
        let state = AppState {
            auth_state: Arc::new(SseAuthState::default()),
            broadcaster: Broadcaster::create(system.clone()),
            controller: Arc::new(RwLock::new(ChannelController::new())),
            file_access: Arc::new(FileAccessState::default()),
            mail_queues: Arc::new(Mutex::new(vec![])),
            pool: pool.clone(),
            shutdown: CancellationToken::new(),
            system,
        };
        let app = routes().with_state(state);
        sqlx::query("UPDATE config_output SET name = 'moq' WHERE config_id = 1 AND active = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!super::super::access::allowed(&pool, 1, None).await);
        for uri in ["/moq/config?channel=1", "/moq/config?channel=999"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/moq/config?channel=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        sqlx::query("UPDATE config_output SET public_moq = 1 WHERE config_id = 1 AND active = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(super::super::access::allowed(&pool, 1, None).await);
        assert!(!super::super::access::allowed(&pool, 999, None).await);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/moq/config?channel=1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(matches!(
            response.status(),
            StatusCode::OK | StatusCode::SERVICE_UNAVAILABLE
        ));
        sqlx::query("UPDATE config_output SET public_moq = 0 WHERE config_id = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(!super::super::access::allowed(&pool, 1, None).await);
        drop(manager);
    }
}
