use actix_web::{get, http::StatusCode, patch, post, put, web, Responder};
use actix_web_grants::{permissions::AuthDetails, proc_macro::has_any_role};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use serde::Serialize;
use simplelog::*;

use crate::api::{
    auth::{create_jwt, Claims},
    errors::ServiceError,
    handles::{
        db_add_user, db_get_settings, db_login, db_role, db_update_settings, db_update_user,
    },
    models::{LoginUser, Settings, User},
    utils::{read_playout_config, Role},
};

use crate::utils::playout_config;

#[derive(Serialize)]
struct ResponseObj<T> {
    message: String,
    status: i32,
    data: Option<T>,
}

#[derive(Debug, Serialize, Clone)]
struct ResponsePlayoutConfig {
    general: Option<playout_config::General>,
    rpc_server: Option<playout_config::RpcServer>,
    mail: Option<playout_config::Mail>,
    logging: Option<playout_config::Logging>,
    processing: Option<playout_config::Processing>,
    ingest: Option<playout_config::Ingest>,
    playlist: Option<playout_config::Playlist>,
    storage: Option<playout_config::Storage>,
    text: Option<playout_config::Text>,
    out: Option<playout_config::Out>,
}

impl ResponsePlayoutConfig {
    fn new() -> Self {
        Self {
            general: None,
            rpc_server: None,
            mail: None,
            logging: None,
            processing: None,
            ingest: None,
            playlist: None,
            storage: None,
            text: None,
            out: None,
        }
    }
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
    details: AuthDetails<Role>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(settings) = db_get_settings(&id).await {
        if let Ok(config) = read_playout_config(&settings.config_path) {
            let mut playout_cfg = ResponsePlayoutConfig::new();

            playout_cfg.playlist = Some(config.playlist);
            playout_cfg.storage = Some(config.storage);
            playout_cfg.text = Some(config.text);

            if details.has_role(&Role::Admin) {
                playout_cfg.general = Some(config.general);
                playout_cfg.rpc_server = Some(config.rpc_server);
                playout_cfg.mail = Some(config.mail);
                playout_cfg.logging = Some(config.logging);
                playout_cfg.processing = Some(config.processing);
                playout_cfg.ingest = Some(config.ingest);
                playout_cfg.out = Some(config.out);

                return Ok(web::Json(playout_cfg));
            }

            return Ok(web::Json(playout_cfg));
        }
    };

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
