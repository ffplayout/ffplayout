use axum::{
    Json,
    extract::{Path, State},
};
use protect_axum::authorities::AuthDetails;

use argon2::{
    Argon2, PasswordHasher,
    password_hash::{SaltString, rand_core::OsRng},
};
use log::*;
use tokio::task;

use crate::{
    api::{
        routes::{AuthUser, ensure_any_authority},
        state::AppState,
    },
    db::{
        handles,
        models::{Role, User},
    },
    utils::errors::ServiceError,
};

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
    Json(data): Json<User>,
) -> Result<&'static str, ServiceError> {
    ensure_any_authority(
        &details,
        &[&Role::GlobalAdmin, &Role::ChannelAdmin, &Role::User],
    )?;
    user.ensure_self_or_admin(id)?;

    let channel_ids = data.channel_ids.clone().unwrap_or_default();
    let mut fields = String::new();

    if let Some(mail) = data.mail.clone() {
        if !fields.is_empty() {
            fields.push_str(", ");
        }

        fields.push_str(&format!("mail = '{mail}'"));
    }

    if !data.password.is_empty() {
        if !fields.is_empty() {
            fields.push_str(", ");
        }

        let password_hash = task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);

            Argon2::default()
                .hash_password(data.password.clone().as_bytes(), &salt)
                .map(|p| p.to_string())
        })
        .await?
        .map_err(|e| ServiceError::Conflict(e.to_string()))?;

        fields.push_str(&format!("password = '{password_hash}'"));
    }

    handles::update_user(&state.pool, id, fields).await?;

    let related_channels = handles::select_related_channels(&state.pool, Some(id)).await?;

    for channel in related_channels {
        if !channel_ids.contains(&channel.id) {
            handles::delete_user_channel(&state.pool, id, channel.id).await?;
        }
    }

    handles::insert_user_channel(&state.pool, id, channel_ids).await?;

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
        Ok(_) => Ok("Delete user success"),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}
