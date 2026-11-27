use axum::{
    Json,
    extract::{Path, State},
};
use protect_axum::authorities::AuthDetails;

use argon2::{Argon2, PasswordHasher, PasswordVerifier, password_hash::phc::PasswordHash};
use log::*;
use serde::Deserialize;
use tokio::task;

use crate::{
    api::{
        auth::revoke_access_tokens,
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{Role, User, validate_new_password},
    },
    utils::errors::ServiceError,
};

/// A user update, with the current password for changes to the own account.
#[derive(Debug, Deserialize)]
pub struct UserUpdate {
    #[serde(flatten)]
    user: User,
    #[serde(default)]
    current_password: Option<String>,
}

async fn verify_current_password(
    state: &AppState,
    id: i32,
    current_password: Option<String>,
) -> Result<(), ServiceError> {
    let rejected = || ServiceError::Forbidden("Current password is incorrect".to_string());
    let password = current_password
        .filter(|password| !password.is_empty())
        .ok_or_else(rejected)?;
    let hash = handles::select_user_password_hash(&state.pool, id).await?;
    let verified = task::spawn_blocking(move || {
        let hash = PasswordHash::new(&hash)?;

        Argon2::default().verify_password(password.as_bytes(), &hash)
    })
    .await?;

    verified.map_err(|_| rejected())
}

/// From here on all request **must** contain the authorization header:\
/// `"Authorization: Bearer <TOKEN>"`
/// **Get current User**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/user' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_user(
    State(state): State<AppState>,
    user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<User>, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;

    match handles::select_user(&state.pool, user.id).await {
        Ok(user) => Ok(Json(user)),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// **Get User by ID**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/user/2' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_by_name(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    _user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<User>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;

    match handles::select_user(&state.pool, id).await {
        Ok(user) => Ok(Json(user)),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

// **Get all User**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/users' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn get_users(
    State(state): State<AppState>,
    _user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<Json<Vec<User>>, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;

    match handles::select_users(&state.pool).await {
        Ok(users) => Ok(Json(users)),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// **Update current User**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/user/1 -H 'Content-Type: application/json' \
/// -d '{"mail": "<MAIL>", "password": "<PASS>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn update_user(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    user: AuthUser,
    details: AuthDetails<Role>,
    Json(update): Json<UserUpdate>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_self_or_admin(id)?;

    let UserUpdate {
        user: data,
        current_password,
    } = update;
    let current = handles::select_user(&state.pool, id).await?;
    let mail_changed = data.mail.is_some() && data.mail != current.mail;

    // Changing the own password or the address that receives the two-factor
    // codes takes over the account. A stolen session must not be enough.
    if user.id == id && (!data.password.is_empty() || mail_changed) {
        verify_current_password(&state, id, current_password).await?;
    }

    // Channel assignments are a privileged operation: a non-admin editing
    // their own account must not be able to grant themselves access to other
    // channels. Only a global admin may change `channel_ids`.
    let update_channels = user.is_global_admin();
    let channel_ids = data.channel_ids.clone().unwrap_or_default();
    let two_factor = user.is_global_admin().then_some(data.two_factor);
    let mail = data.mail.clone();

    let password_hash = if data.password.is_empty() {
        None
    } else {
        validate_new_password(&data.password).map_err(ServiceError::BadRequest)?;

        let password_hash = task::spawn_blocking(move || {
            Argon2::default()
                .hash_password(data.password.clone().as_bytes())
                .map(|p| p.to_string())
        })
        .await?
        .map_err(|e| ServiceError::Conflict(e.to_string()))?;

        Some(password_hash)
    };

    let revoked = handles::update_user_with_channels(
        &state.pool,
        id,
        two_factor,
        mail,
        password_hash,
        update_channels.then_some(channel_ids),
    )
    .await?;

    if let Some(revoked) = revoked {
        revoke_access_tokens(id);
        info!("Password of user {id} changed, revoked {revoked} sessions");
    } else if update_channels && user.id != id {
        // The channels of an access token are fixed when it is issued. The
        // client refreshes it and continues with the current assignments.
        revoke_access_tokens(id);
    }

    Ok("Update Success")
}

/// **Add User**
///
/// ```BASH
/// curl -X POST 'http://127.0.0.1:8787/api/user' -H 'Content-Type: application/json' \
/// -d '{"mail": "<MAIL>", "username": "<USER>", "password": "<PASS>", "role_id": 1, "channel_id": 1}' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn add_user(
    State(state): State<AppState>,
    _user: AuthUser,
    details: AuthDetails<Role>,
    Json(data): Json<User>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;

    if data.username.trim().is_empty() {
        return Err(ServiceError::BadRequest(
            "Username must not be empty".to_string(),
        ));
    }

    validate_new_password(&data.password).map_err(ServiceError::BadRequest)?;

    match handles::insert_user(&state.pool, data).await {
        Ok(..) => Ok("Add User Success"),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

// **Delete User**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/user/2' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
pub async fn remove_user(
    State(state): State<AppState>,
    Path(id): Path<i32>,
    _user: AuthUser,
    details: AuthDetails<Role>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(&details, &[&Role::GlobalAdmin])?;

    match handles::delete_user(&state.pool, id).await {
        Ok(_) => {
            // The account is gone, but its access token would still be valid.
            revoke_access_tokens(id);

            Ok("Delete user success")
        }
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}
