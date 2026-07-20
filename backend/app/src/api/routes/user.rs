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
        let password_hash = task::spawn_blocking(move || {
            let salt = SaltString::generate(&mut OsRng);

            Argon2::default()
                .hash_password(data.password.clone().as_bytes(), &salt)
                .map(|p| p.to_string())
        })
        .await?
        .map_err(|e| ServiceError::Conflict(e.to_string()))?;

        Some(password_hash)
    };

    let mut transaction = state.pool.begin().await?;
    handles::update_user(&mut *transaction, id, two_factor, mail, password_hash).await?;

    if update_channels {
        sqlx::query("DELETE FROM user_channels WHERE user_id = $1")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        handles::insert_user_channel(&mut transaction, id, channel_ids).await?;
    }

    transaction.commit().await?;

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
