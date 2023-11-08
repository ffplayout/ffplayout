/// ### Possible endpoints
///
/// Run the API thru the systemd service, or like:
///
/// ```BASH
/// ffpapi -l 127.0.0.1:8787
/// ```
///
/// For all endpoints an (Bearer) authentication is required.\
/// `{id}` represent the channel id, and at default is 1.
use std::{collections::HashMap, env, fs, path::PathBuf};

use actix_files;
use actix_multipart::Multipart;
use actix_web::{
    delete, get,
    http::{
        header::{ContentDisposition, DispositionType},
        StatusCode,
    },
    patch, post, put, web, HttpRequest, HttpResponse, Responder,
};
use actix_web_grants::{permissions::AuthDetails, proc_macro::has_any_role};

use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use chrono::{DateTime, Datelike, Duration, Local, NaiveDateTime, TimeZone, Utc};
use path_clean::PathClean;
use regex::Regex;
use serde::{Deserialize, Serialize};
use simplelog::*;
use sqlx::{Pool, Sqlite};

use crate::db::{
    handles,
    models::{Channel, LoginUser, TextPreset, User},
};
use crate::utils::{
    channels::{create_channel, delete_channel},
    control::{control_service, control_state, media_info, send_message, ControlParams, Process},
    errors::ServiceError,
    files::{
        browser, create_directory, norm_abs_path, remove_file_or_folder, rename_file, upload,
        MoveObject, PathObject,
    },
    naive_date_time_from_str,
    playlist::{delete_playlist, generate_playlist, read_playlist, write_playlist},
    playout_config, public_path, read_log_file, read_playout_config, system, Role,
};
use crate::{
    api::auth::{create_jwt, Claims},
    utils::control::ProcessControl,
};
use ffplayout_lib::{
    utils::{
        get_date_range, import::import_file, sec_to_time, time_to_sec, JsonPlaylist, PlayoutConfig,
        Template,
    },
    vec_strings,
};

#[derive(Serialize)]
struct ResponseObj<T> {
    message: String,
    status: i32,
    data: Option<T>,
}

#[derive(Serialize)]
struct UserObj<T> {
    message: String,
    user: Option<T>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct DateObj {
    #[serde(default)]
    date: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct FileObj {
    #[serde(default)]
    path: PathBuf,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct PathsObj {
    #[serde(default)]
    paths: Option<Vec<String>>,
    template: Option<Template>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ImportObj {
    #[serde(default)]
    file: PathBuf,
    #[serde(default)]
    date: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ProgramObj {
    #[serde(default = "time_after", deserialize_with = "naive_date_time_from_str")]
    start_after: NaiveDateTime,
    #[serde(default = "time_before", deserialize_with = "naive_date_time_from_str")]
    start_before: NaiveDateTime,
}

fn time_after() -> NaiveDateTime {
    let today = Utc::now();

    chrono::Local
        .with_ymd_and_hms(today.year(), today.month(), today.day(), 0, 0, 0)
        .unwrap()
        .naive_local()
}

fn time_before() -> NaiveDateTime {
    let today = Utc::now();

    chrono::Local
        .with_ymd_and_hms(today.year(), today.month(), today.day(), 23, 59, 59)
        .unwrap()
        .naive_local()
}

#[derive(Debug, Serialize)]
struct ProgramItem {
    source: String,
    start: String,
    r#in: f64,
    out: f64,
    duration: f64,
    category: String,
}

/// #### User Handling
///
/// **Login**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/auth/login/ -H "Content-Type: application/json" \
/// -d '{ "username": "<USER>", "password": "<PASS>" }'
/// ```
/// **Response:**
///
/// ```JSON
/// {
///     "id": 1,
///     "mail": "user@example.org",
///     "username": "<USER>",
///     "token": "<TOKEN>"
/// }
/// ```
#[post("/auth/login/")]
pub async fn login(pool: web::Data<Pool<Sqlite>>, credentials: web::Json<User>) -> impl Responder {
    let conn = pool.into_inner();
    match handles::select_login(&conn, &credentials.username).await {
        Ok(mut user) => {
            let pass = user.password.clone();
            let hash = PasswordHash::new(&pass).unwrap();
            user.password = "".into();
            user.salt = None;

            if Argon2::default()
                .verify_password(credentials.password.as_bytes(), &hash)
                .is_ok()
            {
                let role = handles::select_role(&conn, &user.role_id.unwrap_or_default())
                    .await
                    .unwrap_or(Role::Guest);
                let claims = Claims::new(user.id, user.username.clone(), role.clone());

                if let Ok(token) = create_jwt(claims) {
                    user.token = Some(token);
                };

                info!("user {} login, with role: {role}", credentials.username);

                web::Json(UserObj {
                    message: "login correct!".into(),
                    user: Some(user),
                })
                .customize()
                .with_status(StatusCode::OK)
            } else {
                error!("Wrong password for {}!", credentials.username);
                web::Json(UserObj {
                    message: "Wrong password!".into(),
                    user: None,
                })
                .customize()
                .with_status(StatusCode::FORBIDDEN)
            }
        }
        Err(e) => {
            error!("Login {} failed! {e}", credentials.username);
            web::Json(UserObj {
                message: format!("Login {} failed!", credentials.username),
                user: None,
            })
            .customize()
            .with_status(StatusCode::BAD_REQUEST)
        }
    }
}

/// From here on all request **must** contain the authorization header:\
/// `"Authorization: Bearer <TOKEN>"`

/// **Get current User**
///
/// ```BASH
/// curl -X GET 'http://127.0.0.1:8787/api/user' -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/user")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_user(
    pool: web::Data<Pool<Sqlite>>,
    user: web::ReqData<LoginUser>,
) -> Result<impl Responder, ServiceError> {
    match handles::select_user(&pool.into_inner(), &user.username).await {
        Ok(user) => Ok(web::Json(user)),
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
#[put("/user/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn update_user(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    user: web::ReqData<LoginUser>,
    data: web::Json<User>,
) -> Result<impl Responder, ServiceError> {
    if id.into_inner() == user.id {
        let mut fields = String::new();

        if let Some(mail) = data.mail.clone() {
            fields.push_str(format!("mail = '{mail}'").as_str());
        }

        if !data.password.is_empty() {
            if !fields.is_empty() {
                fields.push_str(", ");
            }

            let salt = SaltString::generate(&mut OsRng);
            let password_hash = Argon2::default()
                .hash_password(data.password.clone().as_bytes(), &salt)
                .unwrap();

            fields.push_str(format!("password = '{password_hash}', salt = '{salt}'").as_str());
        }

        if handles::update_user(&pool.into_inner(), user.id, fields)
            .await
            .is_ok()
        {
            return Ok("Update Success");
        };

        return Err(ServiceError::InternalServerError);
    }

    Err(ServiceError::Unauthorized)
}

/// **Add User**
///
/// ```BASH
/// curl -X POST 'http://127.0.0.1:8787/api/user/' -H 'Content-Type: application/json' \
/// -d '{"mail": "<MAIL>", "username": "<USER>", "password": "<PASS>", "role_id": 1, "channel_id": 1}' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/user/")]
#[has_any_role("Role::Admin", type = "Role")]
async fn add_user(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<User>,
) -> Result<impl Responder, ServiceError> {
    match handles::insert_user(&pool.into_inner(), data.into_inner()).await {
        Ok(_) => Ok("Add User Success"),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// #### ffpapi Settings
///
/// **Get Settings from Channel**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/channel/1 -H "Authorization: Bearer <TOKEN>"
/// ```
///
/// **Response:**
///
/// ```JSON
/// {
///     "id": 1,
///     "name": "Channel 1",
///     "preview_url": "http://localhost/live/preview.m3u8",
///     "config_path": "/etc/ffplayout/ffplayout.yml",
///     "extra_extensions": "jpg,jpeg,png",
///     "service": "ffplayout.service",
///     "utc_offset": "+120"
/// }
/// ```
#[get("/channel/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_channel(&pool.into_inner(), &id).await {
        return Ok(web::Json(channel));
    }

    Err(ServiceError::InternalServerError)
}

/// **Get settings from all Channels**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/channels -H "Authorization: Bearer <TOKEN>"
/// ```
#[get("/channels")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_all_channels(pool: web::Data<Pool<Sqlite>>) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_all_channels(&pool.into_inner()).await {
        return Ok(web::Json(channel));
    }

    Err(ServiceError::InternalServerError)
}

/// **Update Channel**
///
/// ```BASH
/// curl -X PATCH http://127.0.0.1:8787/api/channel/1 -H "Content-Type: application/json" \
/// -d '{ "id": 1, "name": "Channel 1", "preview_url": "http://localhost/live/stream.m3u8", "config_path": "/etc/ffplayout/ffplayout.yml", "extra_extensions": "jpg,jpeg,png"}' \
/// -H "Authorization: Bearer <TOKEN>"
/// ```
#[patch("/channel/{id}")]
#[has_any_role("Role::Admin", type = "Role")]
async fn patch_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<Channel>,
) -> Result<impl Responder, ServiceError> {
    if handles::update_channel(&pool.into_inner(), *id, data.into_inner())
        .await
        .is_ok()
    {
        return Ok("Update Success");
    };

    Err(ServiceError::InternalServerError)
}

/// **Create new Channel**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/channel/ -H "Content-Type: application/json" \
/// -d '{ "name": "Channel 2", "preview_url": "http://localhost/live/channel2.m3u8", "config_path": "/etc/ffplayout/channel2.yml", "extra_extensions": "jpg,jpeg,png", "service": "ffplayout@channel2.service" }' \
/// -H "Authorization: Bearer <TOKEN>"
/// ```
#[post("/channel/")]
#[has_any_role("Role::Admin", type = "Role")]
async fn add_channel(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<Channel>,
) -> Result<impl Responder, ServiceError> {
    match create_channel(&pool.into_inner(), data.into_inner()).await {
        Ok(c) => Ok(web::Json(c)),
        Err(e) => Err(e),
    }
}

/// **Delete Channel**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/channel/2 -H "Authorization: Bearer <TOKEN>"
/// ```
#[delete("/channel/{id}")]
#[has_any_role("Role::Admin", type = "Role")]
async fn remove_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    if delete_channel(&pool.into_inner(), *id).await.is_ok() {
        return Ok("Delete Channel Success");
    }

    Err(ServiceError::InternalServerError)
}

/// #### ffplayout Config
///
/// **Get Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/config/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object from the ffplayout.yml
#[get("/playout/config/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_playout_config(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    _details: AuthDetails<Role>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_channel(&pool.into_inner(), &id).await {
        if let Ok(config) = read_playout_config(&channel.config_path) {
            return Ok(web::Json(config));
        }
    };

    Err(ServiceError::InternalServerError)
}

/// **Update Config**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/playout/config/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
#[put("/playout/config/{id}")]
#[has_any_role("Role::Admin", type = "Role")]
async fn update_playout_config(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<PlayoutConfig>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_channel(&pool.into_inner(), &id).await {
        if let Ok(f) = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(channel.config_path)
        {
            serde_yaml::to_writer(f, &data).unwrap();

            return Ok("Update playout config success.");
        } else {
            return Err(ServiceError::InternalServerError);
        };
    };

    Err(ServiceError::InternalServerError)
}

/// #### Text Presets
///
/// Text presets are made for sending text messages to the ffplayout engine, to overlay them as a lower third.
///
/// **Get all Presets**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/presets/ -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/presets/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_presets(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(presets) = handles::select_presets(&pool.into_inner(), *id).await {
        return Ok(web::Json(presets));
    }

    Err(ServiceError::InternalServerError)
}

/// **Update Preset**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -d '{ "name": "<PRESET NAME>", "text": "<TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[put("/presets/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn update_preset(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<TextPreset>,
) -> Result<impl Responder, ServiceError> {
    if handles::update_preset(&pool.into_inner(), &id, data.into_inner())
        .await
        .is_ok()
    {
        return Ok("Update Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Add new Preset**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/presets/ -H 'Content-Type: application/json' \
/// -d '{ "name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/presets/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn add_preset(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<TextPreset>,
) -> Result<impl Responder, ServiceError> {
    if handles::insert_preset(&pool.into_inner(), data.into_inner())
        .await
        .is_ok()
    {
        return Ok("Add preset Success");
    }

    Err(ServiceError::InternalServerError)
}

/// **Delete Preset**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[delete("/presets/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn delete_preset(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    if handles::delete_preset(&pool.into_inner(), &id)
        .await
        .is_ok()
    {
        return Ok("Delete preset Success");
    }

    Err(ServiceError::InternalServerError)
}

/// ### ffplayout controlling
///
/// here we communicate with the engine for:
/// - jump to last or next clip
/// - reset playlist state
/// - get infos about current, next, last clip
/// - send text to the engine, for overlaying it (as lower third etc.)
///
/// **Send Text to ffplayout**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/control/1/text/ \
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>' \
/// -d '{"text": "Hello from ffplayout", "x": "(w-text_w)/2", "y": "(h-text_h)/2", fontsize": "24", "line_spacing": "4", "fontcolor": "#ffffff", "box": "1", "boxcolor": "#000000", "boxborderw": "4", "alpha": "1.0"}'
/// ```
#[post("/control/{id}/text/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn send_text_message(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<HashMap<String, String>>,
) -> Result<impl Responder, ServiceError> {
    match send_message(&pool.into_inner(), *id, data.into_inner()).await {
        Ok(res) => Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// **Control Playout**
///
/// - next
/// - back
/// - reset
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/control/1/playout/ -H 'Content-Type: application/json'
/// -d '{ "command": "reset" }' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/control/{id}/playout/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn control_playout(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    control: web::Json<ControlParams>,
) -> Result<impl Responder, ServiceError> {
    match control_state(&pool.into_inner(), *id, &control.control).await {
        Ok(res) => Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// **Get current Clip**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/control/1/media/current
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// **Response:**
///
/// ```JSON
/// {
///     "jsonrpc": "2.0",
///     "result": {
///       "current_media": {
///         "category": "",
///         "duration": 154.2,
///         "out": 154.2,
///         "seek": 0.0,
///         "source": "/opt/tv-media/clip.mp4"
///       },
///       "index": 39,
///       "play_mode": "playlist",
///       "played_sec": 67.80771999300123,
///       "remaining_sec": 86.39228000699876,
///       "start_sec": 24713.631999999998,
///       "start_time": "06:51:53.631"
///     },
///     "id": 1
/// }
/// ```
#[get("/control/{id}/media/current")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn media_current(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    match media_info(&pool.into_inner(), *id, "current".into()).await {
        Ok(res) => Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// **Get next Clip**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/control/1/media/next/ -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/control/{id}/media/next")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn media_next(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    match media_info(&pool.into_inner(), *id, "next".into()).await {
        Ok(res) => Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// **Get last Clip**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/control/1/media/last/
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/control/{id}/media/last")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn media_last(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    match media_info(&pool.into_inner(), *id, "last".into()).await {
        Ok(res) => Ok(res.text().await.unwrap_or_else(|_| "Success".into())),
        Err(e) => Err(e),
    }
}

/// #### ffplayout Process Control
///
/// Control ffplayout process, like:
/// - start
/// - stop
/// - restart
/// - status
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/control/1/process/
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// -d '{"command": "start"}'
/// ```
#[post("/control/{id}/process/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn process_control(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    proc: web::Json<Process>,
    engine_process: web::Data<ProcessControl>,
) -> Result<impl Responder, ServiceError> {
    control_service(&pool.into_inner(), *id, &proc.command, Some(engine_process)).await
}

/// #### ffplayout Playlist Operations
///
/// **Get playlist**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playlist/1?date=2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/playlist/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn get_playlist(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    obj: web::Query<DateObj>,
) -> Result<impl Responder, ServiceError> {
    match read_playlist(&pool.into_inner(), *id, obj.date.clone()).await {
        Ok(playlist) => Ok(web::Json(playlist)),
        Err(e) => Err(e),
    }
}

/// **Save playlist**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/playlist/1/
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// --data "{<JSON playlist data>}"
/// ```
#[post("/playlist/{id}/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn save_playlist(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<JsonPlaylist>,
) -> Result<impl Responder, ServiceError> {
    match write_playlist(&pool.into_inner(), *id, data.into_inner()).await {
        Ok(res) => Ok(res),
        Err(e) => Err(e),
    }
}

/// **Generate Playlist**
///
/// A new playlist will be generated and response.
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/playlist/1/generate/2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// /// --data '{ "paths": [<list of paths>] }' # <- data is optional
/// ```
///
/// Or with template:
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/playlist/1/generate/2023-00-05
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// --data '{"template": {"sources": [\
///            {"start": "00:00:00", "duration": "10:00:00", "shuffle": true, "paths": ["path/1", "path/2"]}, \
///            {"start": "10:00:00", "duration": "14:00:00", "shuffle": false, "paths": ["path/3", "path/4"]}]}}'
/// ```
#[post("/playlist/{id}/generate/{date}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn gen_playlist(
    pool: web::Data<Pool<Sqlite>>,
    params: web::Path<(i32, String)>,
    data: Option<web::Json<PathsObj>>,
) -> Result<impl Responder, ServiceError> {
    let (mut config, channel) = playout_config(&pool.into_inner(), &params.0).await?;
    config.general.generate = Some(vec![params.1.clone()]);

    if let Some(obj) = data {
        if let Some(paths) = &obj.paths {
            let mut path_list = vec![];

            for path in paths {
                let (p, _, _) = norm_abs_path(&config.storage.path, path);

                path_list.push(p);
            }

            config.storage.paths = path_list;
        }

        config.general.template = obj.template.clone();
    }

    match generate_playlist(config.to_owned(), channel.name).await {
        Ok(playlist) => Ok(web::Json(playlist)),
        Err(e) => Err(e),
    }
}

/// **Delete Playlist**
///
/// ```BASH
/// curl -X DELETE http://127.0.0.1:8787/api/playlist/1/2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[delete("/playlist/{id}/{date}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn del_playlist(
    pool: web::Data<Pool<Sqlite>>,
    params: web::Path<(i32, String)>,
) -> Result<impl Responder, ServiceError> {
    match delete_playlist(&pool.into_inner(), params.0, &params.1).await {
        Ok(_) => Ok(format!("Delete playlist from {} success!", params.1)),
        Err(e) => Err(e),
    }
}

/// ### Log file
///
/// **Read Log Life**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/log/1
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/log/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn get_log(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    log: web::Query<DateObj>,
) -> Result<impl Responder, ServiceError> {
    read_log_file(&pool.into_inner(), &id, &log.date).await
}

/// ### File Operations
///
/// **Get File/Folder List**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/browse/ -H 'Content-Type: application/json'
/// -d '{ "source": "/" }' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/browse/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn file_browser(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<PathObject>,
) -> Result<impl Responder, ServiceError> {
    match browser(&pool.into_inner(), *id, &data.into_inner()).await {
        Ok(obj) => Ok(web::Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Create Folder**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/create-folder/ -H 'Content-Type: application/json'
/// -d '{"source": "<FOLDER PATH>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/create-folder/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn add_dir(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<PathObject>,
) -> Result<HttpResponse, ServiceError> {
    create_directory(&pool.into_inner(), *id, &data.into_inner()).await
}

/// **Rename File**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/rename/ -H 'Content-Type: application/json'
/// -d '{"source": "<SOURCE>", "target": "<TARGET>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/rename/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn move_rename(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<MoveObject>,
) -> Result<impl Responder, ServiceError> {
    match rename_file(&pool.into_inner(), *id, &data.into_inner()).await {
        Ok(obj) => Ok(web::Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Remove File/Folder**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/remove/ -H 'Content-Type: application/json'
/// -d '{"source": "<SOURCE>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/remove/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn remove(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<PathObject>,
) -> Result<impl Responder, ServiceError> {
    match remove_file_or_folder(&pool.into_inner(), *id, &data.into_inner().source).await {
        Ok(obj) => Ok(web::Json(obj)),
        Err(e) => Err(e),
    }
}

/// **Upload File**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/file/1/upload/ -H 'Authorization: Bearer <TOKEN>'
/// -F "file=@file.mp4"
/// ```
#[put("/file/{id}/upload/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn save_file(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    payload: Multipart,
    obj: web::Query<FileObj>,
) -> Result<HttpResponse, ServiceError> {
    upload(&pool.into_inner(), *id, payload, &obj.path, false).await
}

/// **Get File**
///
/// Can be used for preview video files
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/file/1/path/to/file.mp4
/// ```
#[get("/file/{id}/{filename:.*}")]
async fn get_file(
    pool: web::Data<Pool<Sqlite>>,
    req: HttpRequest,
) -> Result<actix_files::NamedFile, ServiceError> {
    let id: i32 = req.match_info().query("id").parse()?;
    let (config, _) = playout_config(&pool.into_inner(), &id).await?;
    let storage_path = config.storage.path;
    let file_path = req.match_info().query("filename");
    let (path, _, _) = norm_abs_path(&storage_path, file_path);
    let file = actix_files::NamedFile::open(path)?;

    Ok(file
        .use_last_modified(true)
        .set_content_disposition(ContentDisposition {
            disposition: DispositionType::Attachment,
            parameters: vec![],
        }))
}

/// **Get Public**
///
/// Can be used for HLS Playlist and other static files in public folder
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/live/stream.m3u8
/// ```
#[get("/{public:((live|preview|public).*|.*(ts|m3u8))}")]
async fn get_public(public: web::Path<String>) -> Result<actix_files::NamedFile, ServiceError> {
    let public_path = public_path();

    let absolute_path = if public_path.is_absolute() {
        public_path.to_path_buf()
    } else {
        env::current_dir()?.join(public_path)
    }
    .clean();

    let path = absolute_path.join(public.as_str());
    let file = actix_files::NamedFile::open(path)?;

    Ok(file
        .use_last_modified(true)
        .set_content_disposition(ContentDisposition {
            disposition: DispositionType::Attachment,
            parameters: vec![],
        }))
}

/// **Import playlist**
///
/// Import text/m3u file and convert it to a playlist
/// lines with leading "#" will be ignore
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/file/1/import/ -H 'Authorization: Bearer <TOKEN>'
/// -F "file=@list.m3u"
/// ```
#[put("/file/{id}/import/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn import_playlist(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    payload: Multipart,
    obj: web::Query<ImportObj>,
) -> Result<HttpResponse, ServiceError> {
    let file = obj.file.file_name().unwrap_or_default();
    let path = env::temp_dir().join(file);
    let (config, _) = playout_config(&pool.clone().into_inner(), &id).await?;
    let channel = handles::select_channel(&pool.clone().into_inner(), &id).await?;

    upload(&pool.into_inner(), *id, payload, &path, true).await?;
    import_file(&config, &obj.date, Some(channel.name), &path)?;

    fs::remove_file(path)?;

    Ok(HttpResponse::Ok().into())
}

/// **Program info**
///
/// Get program infos about given date, or current day
///
/// Examples:
///
/// * get program from current day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1/ -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get a program range between two dates
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1/?start_after=2022-11-13T12:00:00&start_before=2022-11-20T11:59:59 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// * get program from give day
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/program/1/?start_after=2022-11-13T10:00:00 \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/program/{id}/")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
async fn get_program(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    obj: web::Query<ProgramObj>,
) -> Result<impl Responder, ServiceError> {
    let (config, _) = playout_config(&pool.clone().into_inner(), &id).await?;
    let start_sec = config.playlist.start_sec.unwrap();
    let mut days = 0;
    let mut program = vec![];
    let after = obj.start_after;
    let mut before = obj.start_before;

    if after > before {
        before = chrono::Local
            .with_ymd_and_hms(after.year(), after.month(), after.day(), 23, 59, 59)
            .unwrap()
            .naive_local()
    }

    if start_sec > time_to_sec(&after.format("%H:%M:%S").to_string()) {
        days = 1;
    }

    let date_range = get_date_range(&vec_strings![
        (after - Duration::days(days)).format("%Y-%m-%d"),
        "-",
        before.format("%Y-%m-%d")
    ]);

    for date in date_range {
        let conn = pool.clone().into_inner();
        let mut naive = NaiveDateTime::parse_from_str(
            &format!("{date} {}", sec_to_time(start_sec)),
            "%Y-%m-%d %H:%M:%S%.3f",
        )
        .unwrap();

        let playlist = match read_playlist(&conn, *id, date.clone()).await {
            Ok(p) => p,
            Err(e) => {
                error!("Error in Playlist from {date}: {e}");
                continue;
            }
        };

        for item in playlist.program {
            let start: DateTime<Local> = Local.from_local_datetime(&naive).unwrap();

            let source = match Regex::new(&config.text.regex)
                .ok()
                .and_then(|r| r.captures(&item.source))
            {
                Some(t) => t[1].to_string(),
                None => item.source,
            };

            let p_item = ProgramItem {
                source,
                start: start.format("%Y-%m-%d %H:%M:%S%.3f%:z").to_string(),
                r#in: item.seek,
                out: item.out,
                duration: item.duration,
                category: item.category,
            };

            if naive >= after && naive <= before {
                program.push(p_item);
            }

            naive += Duration::milliseconds(((item.out - item.seek) * 1000.0) as i64);
        }
    }

    Ok(web::Json(program))
}

/// ### System Statistics
///
/// Get statistics about CPU, Ram, Disk, etc. usage.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/system
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/system/{id}")]
#[has_any_role("Role::Admin", "Role::User", type = "Role")]
pub async fn get_system_stat(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    let (config, _) = playout_config(&pool.clone().into_inner(), &id).await?;

    let stat = web::block(move || system::stat(config)).await?;

    Ok(web::Json(stat))
}
