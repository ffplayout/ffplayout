use std::collections::HashMap;

use actix_web::{delete, get, http::StatusCode, patch, post, put, web, Responder};
use actix_web_grants::{permissions::AuthDetails, proc_macro::has_any_role};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use serde::Serialize;
use simplelog::*;

use crate::{
    api::{
        auth::{create_jwt, Claims},
        control::{control_state, media_info, send_message},
        errors::ServiceError,
        files::{browser, PathObject},
        handles::{
            db_add_preset, db_add_user, db_get_presets, db_get_settings, db_login, db_role,
            db_update_preset, db_update_settings, db_update_user,
        },
        models::{LoginUser, Settings, TextPreset, User},
        playlist::{delete_playlist, read_playlist, write_playlist},
        utils::{read_playout_config, Role},
    },
    utils::{JsonPlaylist, PlayoutConfig},
};

#[derive(Serialize)]
struct ResponseObj<T> {
    message: String,
    status: i32,
    data: Option<T>,
}

/// curl -X GET http://127.0.0.1:8080/api/settings/1 -H "Authorization: Bearer <TOKEN>"
#[get("/settings/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_settings(id: web::Path<i64>) -> Result<impl Responder, ServiceError> {
    if let Ok(settings) = db_get_settings(&id).await {
        return Ok(web::Json(ResponseObj {
            message: format!("Settings from {}", settings.channel_name),
            status: 200,
            data: Some(settings),
        }));
    }

    Err(ServiceError::InternalServerError)
}

/// curl -X PATCH http://127.0.0.1:8080/api/settings/1 -H "Content-Type: application/json"  \
/// --data '{"id":1,"channel_name":"Channel 1","preview_url":"http://localhost/live/stream.m3u8", \
/// "config_path":"/etc/ffplayout/ffplayout.yml","extra_extensions":".jpg,.jpeg,.png"}' \
/// -H "Authorization: Bearer <TOKEN>"
#[patch("/settings/{id}")]
#[has_any_role("Role::Admin", type = "Role")]
async fn patch_settings(
    id: web::Path<i64>,
    data: web::Json<Settings>,
) -> Result<impl Responder, ServiceError> {
    if db_update_settings(*id, data.into_inner()).await.is_ok() {
        return Ok("Update Success");
    };

    Err(ServiceError::InternalServerError)
}

/// curl -X GET http://localhost:8080/api/playout/config/1 --header 'Authorization: <TOKEN>'
#[get("/playout/config/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_playout_config(
    id: web::Path<i64>,
    _details: AuthDetails<Role>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(settings) = db_get_settings(&id).await {
        if let Ok(config) = read_playout_config(&settings.config_path) {
            return Ok(web::Json(config));
        }
    };

    Err(ServiceError::InternalServerError)
}

/// curl -X PUT http://localhost:8080/api/playout/config/1  -H "Content-Type: application/json" \
/// --data { <CONFIG DATA> } --header 'Authorization: <TOKEN>'
#[put("/playout/config/{id}")]
#[has_any_role("Role::Admin", type = "Role")]
async fn update_playout_config(
    id: web::Path<i64>,
    data: web::Json<PlayoutConfig>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(settings) = db_get_settings(&id).await {
        if let Ok(f) = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&settings.config_path)
        {
            serde_yaml::to_writer(f, &data).unwrap();

            return Ok("Update playout config success.");
        } else {
            return Err(ServiceError::InternalServerError);
        };
    };

    Err(ServiceError::InternalServerError)
}

/// curl -X PUT http://localhost:8080/api/presets/ --header 'Content-Type: application/json' \
/// --data '{"email": "<EMAIL>", "password": "<PASS>"}' --header 'Authorization: <TOKEN>'
#[get("/presets/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_presets() -> Result<impl Responder, ServiceError> {
    if let Ok(presets) = db_get_presets().await {
        return Ok(web::Json(presets));
    }

    Err(ServiceError::InternalServerError)
}

/// curl -X PUT http://localhost:8080/api/presets/1 --header 'Content-Type: application/json' \
/// --data '{"name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, \
/// "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0}}' \
/// --header 'Authorization: <TOKEN>'
#[put("/presets/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn update_preset(
    id: web::Path<i64>,
    data: web::Json<TextPreset>,
) -> Result<impl Responder, ServiceError> {
    if db_update_preset(&id, data.into_inner()).await.is_ok() {
        return Ok("Update Success");
    }

    Err(ServiceError::InternalServerError)
}

/// curl -X POST http://localhost:8080/api/presets/ --header 'Content-Type: application/json' \
/// --data '{"name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, \
/// "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0}}' \
/// --header 'Authorization: <TOKEN>'
#[post("/presets/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn add_preset(data: web::Json<TextPreset>) -> Result<impl Responder, ServiceError> {
    if db_add_preset(data.into_inner()).await.is_ok() {
        return Ok("Add preset Success");
    }

    Err(ServiceError::InternalServerError)
}

/// curl -X PUT http://localhost:8080/api/user/1 --header 'Content-Type: application/json' \
/// --data '{"email": "<EMAIL>", "password": "<PASS>"}' --header 'Authorization: <TOKEN>'
#[put("/user/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn update_user(
    id: web::Path<i64>,
    user: web::ReqData<LoginUser>,
    data: web::Json<User>,
) -> Result<impl Responder, ServiceError> {
    if id.into_inner() == user.id {
        let mut fields = String::new();

        if let Some(email) = data.email.clone() {
            fields.push_str(format!("email = '{email}'").as_str());
        }

        if !data.password.is_empty() {
            if !fields.is_empty() {
                fields.push_str(", ");
            }

            let salt = SaltString::generate(&mut OsRng);
            let password_hash = Argon2::default()
                .hash_password(data.password.clone().as_bytes(), &salt)
                .unwrap();

            fields.push_str(format!("password = '{}', salt = '{salt}'", password_hash).as_str());
        }

        if db_update_user(user.id, fields).await.is_ok() {
            return Ok("Update Success");
        };

        return Err(ServiceError::InternalServerError);
    }

    Err(ServiceError::Unauthorized)
}

/// curl -X POST 'http://localhost:8080/api/user/' --header 'Content-Type: application/json' \
/// -d '{"email": "<EMAIL>", "username": "<USER>", "password": "<PASS>", "role_id": 1}' \
/// --header 'Authorization: Bearer <TOKEN>'
#[post("/user/")]
#[has_any_role("Role::Admin", type = "Role")]
async fn add_user(data: web::Json<User>) -> Result<impl Responder, ServiceError> {
    match db_add_user(data.into_inner()).await {
        Ok(_) => Ok("Add User Success"),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// curl -X POST http://127.0.0.1:8080/auth/login/ -H "Content-Type: application/json" \
/// -d '{"username": "<USER>", "password": "<PASS>" }'
#[post("/auth/login/")]
pub async fn login(credentials: web::Json<User>) -> impl Responder {
    match db_login(&credentials.username).await {
        Ok(mut user) => {
            let pass = user.password.clone();
            let hash = PasswordHash::new(&pass).unwrap();
            user.password = "".into();
            user.salt = None;

            if Argon2::default()
                .verify_password(credentials.password.as_bytes(), &hash)
                .is_ok()
            {
                let role = db_role(&user.role_id.unwrap_or_default())
                    .await
                    .unwrap_or_else(|_| "guest".to_string());
                let claims = Claims::new(user.id, user.username.clone(), role.clone());

                if let Ok(token) = create_jwt(claims) {
                    user.token = Some(token);
                };

                info!("user {} login, with role: {role}", credentials.username);

                web::Json(ResponseObj {
                    message: "login correct!".into(),
                    status: 200,
                    data: Some(user),
                })
                .customize()
                .with_status(StatusCode::OK)
            } else {
                error!("Wrong password for {}!", credentials.username);
                web::Json(ResponseObj {
                    message: "Wrong password!".into(),
                    status: 403,
                    data: None,
                })
                .customize()
                .with_status(StatusCode::FORBIDDEN)
            }
        }
        Err(e) => {
            error!("Login {} failed! {e}", credentials.username);
            return web::Json(ResponseObj {
                message: format!("Login {} failed!", credentials.username),
                status: 400,
                data: None,
            })
            .customize()
            .with_status(StatusCode::BAD_REQUEST);
        }
    }
}

/// ----------------------------------------------------------------------------
/// ffplayout process controlling
///
/// here we communicate with the engine for:
/// - jump to last or next clip
/// - reset playlist state
/// - get infos about current, next, last clip
/// - send text the the engine, for overlaying it (as lower third etc.)
/// ----------------------------------------------------------------------------

/// curl -X POST http://localhost:8080/api/control/1/text/ \
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>' \
/// --data '{"text": "Hello from ffplayout", "x": "(w-text_w)/2", "y": "(h-text_h)/2", \
///     "fontsize": "24", "line_spacing": "4", "fontcolor": "#ffffff", "box": "1", \
///     "boxcolor": "#000000", "boxborderw": "4", "alpha": "1.0"}'
#[post("/control/{id}/text/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn send_text_message(
    id: web::Path<i64>,
    data: web::Json<HashMap<String, String>>,
) -> Result<impl Responder, ServiceError> {
    match send_message(*id, data.into_inner()).await {
        Ok(res) => return Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// curl -X POST http://localhost:8080/api/control/1/playout/next/
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[post("/control/{id}/playout/next/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn jump_to_next(id: web::Path<i64>) -> Result<impl Responder, ServiceError> {
    match control_state(*id, "next".into()).await {
        Ok(res) => return Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// curl -X POST http://localhost:8080/api/control/1/playout/back/
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[post("/control/{id}/playout/back/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn jump_to_last(id: web::Path<i64>) -> Result<impl Responder, ServiceError> {
    match control_state(*id, "back".into()).await {
        Ok(res) => return Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// curl -X POST http://localhost:8080/api/control/1/playout/reset/
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[post("/control/{id}/playout/reset/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn reset_playout(id: web::Path<i64>) -> Result<impl Responder, ServiceError> {
    match control_state(*id, "reset".into()).await {
        Ok(res) => return Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// curl -X GET http://localhost:8080/api/control/1/media/current/
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[get("/control/{id}/media/current")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn media_current(id: web::Path<i64>) -> Result<impl Responder, ServiceError> {
    match media_info(*id, "current".into()).await {
        Ok(res) => return Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// curl -X GET http://localhost:8080/api/control/1/media/next/
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[get("/control/{id}/media/next")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn media_next(id: web::Path<i64>) -> Result<impl Responder, ServiceError> {
    match media_info(*id, "next".into()).await {
        Ok(res) => return Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// curl -X GET http://localhost:8080/api/control/1/media/last/
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[get("/control/{id}/media/last")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn media_last(id: web::Path<i64>) -> Result<impl Responder, ServiceError> {
    match media_info(*id, "last".into()).await {
        Ok(res) => return Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// ----------------------------------------------------------------------------
/// ffplayout playlist operations
///
/// ----------------------------------------------------------------------------

/// curl -X GET http://localhost:8080/api/playlist/1/2022-06-20
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[get("/playlist/{id}/{date}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn get_playlist(
    params: web::Path<(i64, String)>,
) -> Result<impl Responder, ServiceError> {
    match read_playlist(params.0, params.1.clone()).await {
        Ok(playlist) => Ok(web::Json(playlist)),
        Err(e) => Err(e),
    }
}

/// curl -X POST http://localhost:8080/api/playlist/1/
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
/// -- data "{<JSON playlist data>}"
#[post("/playlist/{id}/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn save_playlist(
    id: web::Path<i64>,
    data: web::Json<JsonPlaylist>,
) -> Result<impl Responder, ServiceError> {
    match write_playlist(*id, data.into_inner()).await {
        Ok(res) => Ok(res),
        Err(e) => Err(e),
    }
}

/// curl -X DELETE http://localhost:8080/api/playlist/1/2022-06-20
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[delete("/playlist/{id}/{date}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn del_playlist(
    params: web::Path<(i64, String)>,
) -> Result<impl Responder, ServiceError> {
    match delete_playlist(params.0, &params.1).await {
        Ok(_) => Ok(format!("Delete playlist from {} success!", params.1)),
        Err(e) => Err(e),
    }
}

/// ----------------------------------------------------------------------------
/// file operations
///
/// ----------------------------------------------------------------------------

/// curl -X get http://localhost:8080/api/file/1/browse
/// --header 'Content-Type: application/json' --header 'Authorization: <TOKEN>'
#[post("/file/{id}/browse/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn file_browser(
    id: web::Path<i64>,
    data: web::Json<PathObject>,
) -> Result<impl Responder, ServiceError> {
    match browser(*id, &data.into_inner()).await {
        Ok(obj) => Ok(web::Json(obj)),
        Err(e) => Err(e),
    }
}
