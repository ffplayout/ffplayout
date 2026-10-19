//! Channel-scoped relay admissions. Private credentials never enter browser URLs.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use moq_auth::{Grant, Patterns, lease};
use moq_relay::auth::{Admissions, Error};
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    api::{auth::decode_jwt, routes::AuthUser},
    db::handles,
    player::controller::ChannelController,
    utils::errors::ServiceError,
};

#[derive(Clone)]
struct Ticket {
    channel: i32,
    bearer: String,
    expires: Instant,
}

#[derive(Default)]
pub(super) struct Tickets(Mutex<HashMap<String, Ticket>>);

impl Tickets {
    pub(super) fn issue(&self, channel: i32, bearer: String) -> Result<String, ServiceError> {
        let mut tickets = self
            .0
            .lock()
            .map_err(|_| ServiceError::InternalServerError)?;
        tickets.retain(|_, ticket| ticket.expires > Instant::now());
        // Bound outstanding private handshake tickets as well as their lifetime.
        if tickets.len() >= 4096 {
            return Err(ServiceError::ServiceUnavailable(
                "Too many MoQ connections".into(),
            ));
        }
        let key = Uuid::new_v4().to_string();
        tickets.insert(
            key.clone(),
            Ticket {
                channel,
                bearer,
                expires: Instant::now() + Duration::from_secs(60),
            },
        );

        Ok(key)
    }

    fn take(&self, key: &str, channel: i32) -> Option<String> {
        let ticket = self.0.lock().ok()?.remove(key)?;

        (ticket.channel == channel && ticket.expires > Instant::now()).then_some(ticket.bearer)
    }
}

pub(super) async fn allowed(pool: &SqlitePool, channel: i32, bearer: Option<&str>) -> bool {
    let Ok(Some(public)) = handles::moq_access(pool, channel).await else {
        return false;
    };
    if let Some(bearer) = bearer {
        let Ok(claims) = decode_jwt(bearer).await else {
            return false;
        };
        let user = AuthUser {
            id: claims.id,
            channels: claims.channels,
            role: claims.role,
        };

        return user.ensure_channel_or_admin(channel).is_ok();
    }

    public
}

fn grant(channel: i32) -> Grant {
    let subscribe = [format!("channel-{channel}.hang")
        .parse()
        .expect("numeric channel path")]
    .into_iter()
    .collect();
    let mut grant = Grant::new(Patterns::new(), subscribe);
    // Never inherit a caller-controlled dialed path or a wildcard subscription.
    grant.root = Some("channels".into());

    grant
}

pub(super) fn run(
    mut admissions: Admissions,
    pool: SqlitePool,
    tickets: Arc<Tickets>,
    stopped: CancellationToken,
    controller: Arc<RwLock<ChannelController>>,
) {
    tokio::spawn(async move {
        loop {
            let admission = tokio::select! {
                _ = stopped.cancelled() => break,
                admission = admissions.next() => match admission { Some(admission) => admission, None => break },
            };
            let controller = controller.clone();
            let pool = pool.clone();
            let tickets = tickets.clone();
            let stopped = stopped.clone();
            tokio::spawn(async move {
                let params: HashMap<String, String> = url::form_urlencoded::parse(
                    admission.request.query.as_deref().unwrap_or("").as_bytes(),
                )
                .into_owned()
                .collect();
                let channel = params
                    .get("channel")
                    .and_then(|channel| channel.parse::<i32>().ok())
                    .filter(|id| *id > 0);
                let Some(channel) = channel else {
                    admission.refuse(Error::Refused);
                    return;
                };
                // Explicit credentials are never silently downgraded to anonymous access.
                let bearer = match params.get("ticket") {
                    Some(key) => match tickets.take(key, channel) {
                        Some(bearer) => Some(bearer),
                        None => {
                            admission.refuse(Error::Refused);
                            return;
                        }
                    },
                    None => None,
                };
                if admission.request.token.is_some()
                    || !allowed(&pool, channel, bearer.as_deref()).await
                {
                    admission.refuse(Error::Refused);
                    return;
                }
                let Some(manager) = controller.read().await.get(channel) else {
                    admission.refuse(Error::Refused);
                    return;
                };
                // The lease outlives the HTTP handshake and covers every relay
                // transport, including direct QUIC and WebTransport sessions.
                let _viewer = manager.moq_demand.connect();
                let (producer, consumer) = lease::Producer::new(grant(channel));
                admission.grant(consumer);
                // Revoke existing anonymous sessions after a public opt-out, including
                // direct QUIC/WebTransport connections bypassing the HTTP proxy.
                loop {
                    tokio::select! {
                        _ = stopped.cancelled() => break,
                        _ = producer.closed() => break,
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {
                            if !allowed(&pool, channel, bearer.as_deref()).await { break; }
                        }
                    }
                }
                producer.revoke(lease::Reason::Refused);
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) async fn bearer(channels: &[i32]) -> String {
        use crate::{
            api::auth::{Claims, encode_jwt},
            db::{GLOBAL_SETTINGS, models::GlobalSettings},
        };
        GLOBAL_SETTINGS.get_or_init(|| GlobalSettings {
            secret: Some("moq-access-test-only".into()),
            ..GlobalSettings::default()
        });
        let now = chrono::Utc::now().timestamp();
        let claims: Claims = serde_json::from_value(serde_json::json!({
            "id": 1, "channels": channels, "username": "viewer", "role": "user",
            "token_type": "access", "iat": now, "exp": now + 60
        }))
        .unwrap();

        encode_jwt(claims).await.unwrap()
    }

    #[tokio::test]
    async fn relay_admissions_enforce_channel_scope_and_revoke_public_access() {
        let (manager, pool) = crate::test_support::channel_manager().await;
        sqlx::query("UPDATE config_output SET name = 'moq', public_moq = 1 WHERE config_id = 1 AND active = 1").execute(&pool).await.unwrap();
        let stopped = CancellationToken::new();
        let tickets = Arc::new(Tickets::default());
        let (auth, admissions) = moq_relay::auth::Auth::embedded("ffplayout-test");
        let mut channels = crate::player::controller::ChannelController::new();
        channels.add(manager.clone());
        let controller = Arc::new(tokio::sync::RwLock::new(channels));
        run(
            admissions,
            pool.clone(),
            tickets.clone(),
            stopped.clone(),
            controller,
        );
        let request = |channel| {
            let mut request = auth.request(moq_auth::Transport::WebSocket, "/arbitrary-path");
            request.query = Some(format!("channel={channel}"));
            request
        };
        assert!(auth.admit(request(2)).await.is_err());
        assert!(!manager.moq_demand.active());
        let lease = auth.admit(request(1)).await.unwrap();
        assert!(manager.moq_demand.active());
        assert!(lease.token().publish.is_empty());
        assert_eq!(lease.token().root.as_str(), "channels");
        assert_eq!(
            serde_json::to_value(&lease.token().subscribe).unwrap(),
            serde_json::json!(["channel-1.hang"])
        );
        sqlx::query("UPDATE config_output SET public_moq = 0 WHERE config_id = 1")
            .execute(&pool)
            .await
            .unwrap();
        assert!(auth.admit(request(1)).await.is_err());
        let good = bearer(&[1]).await;
        assert!(allowed(&pool, 1, Some(&good)).await);
        assert!(!allowed(&pool, 1, Some(&bearer(&[2]).await)).await);
        let key = tickets.issue(1, good).unwrap();
        let mut private = request(1);
        private.query = Some(format!("channel=1&ticket={key}"));
        let private_lease = auth.admit(private).await.unwrap();
        assert_eq!(private_lease.token().root.as_str(), "channels");
        // Every transport uses these admissions, including direct relay listeners.
        let mut expired = request(1);
        expired.query = Some(format!("channel=1&ticket={key}"));
        assert!(auth.admit(expired).await.is_err());
        // Lease revocation is driven by the decider even without another HTTP request.
        let mut lease = lease;
        assert!(
            tokio::time::timeout(Duration::from_secs(7), lease.ended())
                .await
                .is_ok()
        );
        stopped.cancel();
        drop(private_lease);
        drop(manager);
    }

    #[test]
    fn tickets_are_single_use_and_bound_to_one_channel() {
        let tickets = Tickets::default();
        let key = tickets.issue(1, "private credential".into()).unwrap();
        assert!(tickets.take(&key, 2).is_none());
        assert!(tickets.take(&key, 1).is_none());
        let key = tickets.issue(1, "private credential".into()).unwrap();
        assert_eq!(tickets.take(&key, 1).as_deref(), Some("private credential"));
        assert!(tickets.take(&key, 1).is_none());
    }

    #[test]
    fn grants_allow_only_one_broadcast_and_no_publishing() {
        let grant = grant(12);
        assert!(grant.publish.is_empty());
        assert_eq!(
            serde_json::to_value(&grant).unwrap()["subscribe"],
            serde_json::json!(["channel-12.hang"])
        );
        assert_eq!(grant.root.as_deref(), Some("channels"));
    }
}
