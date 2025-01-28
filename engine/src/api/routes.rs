/// ### Possible endpoints
///
/// Run the API thru the systemd service, or like:
///
/// ```BASH
/// ffplayout -l 127.0.0.1:8787
/// ```
///
/// For all endpoints an (Bearer) authentication is required.\
/// `{id}` represent the channel id, and at default is 1.
use std::{
    env,
    path::{Path, PathBuf},
    sync::{atomic::Ordering, Arc},
};

use actix_files;
use actix_multipart::Multipart;
use actix_web::{
    delete, get,
    http::header::{ContentDisposition, DispositionType},
    patch, post, put, web, HttpRequest, HttpResponse, Responder,
};
use actix_web_grants::{authorities::AuthDetails, proc_macro::protect};

use argon2::{
    password_hash::{rand_core::OsRng, SaltString},
    Argon2, PasswordHasher,
};
use chrono::{DateTime, Datelike, Local, NaiveDateTime, TimeDelta, TimeZone, Utc};
use log::*;
use path_clean::PathClean;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sqlx::{Pool, Sqlite};
use tokio::{fs, sync::Mutex};

use crate::{
    api::auth::{self, Credentials, TokenRefreshRequest},
    db::{
        handles,
        models::{Channel, Role, TextPreset, User, UserMeta},
    },
    file::{norm_abs_path, MoveObject, PathObject},
    player::{
        controller::ChannelController,
        utils::{
            get_data_map, get_date_range, import::import_file, sec_to_time, time_to_sec,
            JsonPlaylist,
        },
    },
    utils::{
        advanced_config::AdvancedConfig,
        channels::{create_channel, delete_channel},
        config::{get_config, PlayoutConfig, Template},
        control::{control_state, send_message, ControlParams, Process, ProcessCtl},
        errors::ServiceError,
        mail::MailQueue,
        naive_date_time_from_str,
        playlist::{delete_playlist, generate_playlist, read_playlist, write_playlist},
        public_path, read_log_file, system, TextFilter,
    },
    vec_strings,
};

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
    title: Option<String>,
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
///     "access": "<ACCESS TOKEN>",
///     "refresh": "<REFRESH TOKEN>"
/// }
/// ```
#[post("/login/")]
pub async fn login(
    pool: web::Data<Pool<Sqlite>>,
    credentials: web::Json<Credentials>,
) -> Result<impl Responder, ServiceError> {
    auth::authorize(&pool.into_inner(), credentials.into_inner()).await
}

/// **Refresh token**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/auth/refresh/ -H "Content-Type: application/json" \
/// -d '{ "refresh": "REFRESH TOKEN>" }'
/// ```
/// **Response:**
///
/// ```JSON
/// {
///     "access": "<ACCESS TOKEN>",
/// }
/// ```
#[post("/refresh/")]
pub async fn refresh(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<TokenRefreshRequest>,
) -> Result<impl Responder, ServiceError> {
    auth::refresh(&pool.into_inner(), data.into_inner()).await
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role"
)]
async fn get_user(
    pool: web::Data<Pool<Sqlite>>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    match handles::select_user(&pool, user.id).await {
        Ok(user) => Ok(web::Json(user)),
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
#[get("/user/{id}")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn get_by_name(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    match handles::select_user(&pool, *id).await {
        Ok(user) => Ok(web::Json(user)),
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
#[get("/users")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn get_users(pool: web::Data<Pool<Sqlite>>) -> Result<impl Responder, ServiceError> {
    match handles::select_users(&pool).await {
        Ok(users) => Ok(web::Json(users)),
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "*id == user.id || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_user(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<User>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
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

        let password_hash = web::block(move || {
            let salt = SaltString::generate(&mut OsRng);

            let argon = Argon2::default()
                .hash_password(data.password.clone().as_bytes(), &salt)
                .map(|p| p.to_string());

            argon
        })
        .await?
        .map_err(|e| ServiceError::Conflict(e.to_string()))?;

        fields.push_str(&format!("password = '{password_hash}'"));
    }

    handles::update_user(&pool, *id, fields).await?;

    let related_channels = handles::select_related_channels(&pool, Some(*id)).await?;

    for channel in related_channels {
        if !channel_ids.contains(&channel.id) {
            handles::delete_user_channel(&pool, *id, channel.id).await?;
        }
    }

    handles::insert_user_channel(&pool, *id, channel_ids).await?;

    Ok("Update Success")
}

/// **Add User**
///
/// ```BASH
/// curl -X POST 'http://127.0.0.1:8787/api/user/' -H 'Content-Type: application/json' \
/// -d '{"mail": "<MAIL>", "username": "<USER>", "password": "<PASS>", "role_id": 1, "channel_id": 1}' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/user/")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn add_user(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<User>,
) -> Result<impl Responder, ServiceError> {
    match handles::insert_user(&pool, data.into_inner()).await {
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
#[delete("/user/{id}")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn remove_user(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
) -> Result<impl Responder, ServiceError> {
    match handles::delete_user(&pool, *id).await {
        Ok(_) => return Ok("Delete user success"),
        Err(e) => {
            error!("{e}");
            Err(ServiceError::InternalServerError)
        }
    }
}

/// #### Settings
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
///     "extra_extensions": "jpg,jpeg,png",
///     "utc_offset": "+120"
/// }
/// ```
#[get("/channel/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_channel(&pool, &id).await {
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role"
)]
async fn get_all_channels(
    pool: web::Data<Pool<Sqlite>>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(channel) = handles::select_related_channels(&pool, Some(user.id)).await {
        return Ok(web::Json(channel));
    }

    Err(ServiceError::InternalServerError)
}

/// **Update Channel**
///
/// ```BASH
/// curl -X PATCH http://127.0.0.1:8787/api/channel/1 -H "Content-Type: application/json" \
/// -d '{ "id": 1, "name": "Channel 1", "preview_url": "http://localhost/live/stream.m3u8", "extra_extensions": "jpg,jpeg,png"}' \
/// -H "Authorization: Bearer <TOKEN>"
/// ```
#[patch("/channel/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn patch_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<Channel>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel {id} not found!")))?;
    let mut data = data.into_inner();

    if !role.has_authority(&Role::GlobalAdmin) {
        let channel = handles::select_channel(&pool, &id).await?;

        data.public = channel.public;
        data.playlists = channel.playlists;
        data.storage = channel.storage;
    }

    handles::update_channel(&pool, *id, data.clone()).await?;
    let new_config = get_config(&pool, *id).await?;

    manager.update_config(new_config).await;
    manager.update_channel(&data).await;

    Ok("Update Success")
}

/// **Create new Channel**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/channel/ -H "Content-Type: application/json" \
/// -d '{ "name": "Channel 2", "preview_url": "http://localhost/live/channel2.m3u8", "extra_extensions": "jpg,jpeg,png" }' \
/// -H "Authorization: Bearer <TOKEN>"
/// ```
#[post("/channel/")]
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn add_channel(
    pool: web::Data<Pool<Sqlite>>,
    data: web::Json<Channel>,
    controllers: web::Data<Mutex<ChannelController>>,
    queue: web::Data<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> Result<impl Responder, ServiceError> {
    match create_channel(
        &pool,
        controllers.into_inner(),
        queue.into_inner(),
        data.into_inner(),
    )
    .await
    {
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
#[protect("Role::GlobalAdmin", ty = "Role")]
async fn remove_channel(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    queue: web::Data<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> Result<impl Responder, ServiceError> {
    delete_channel(&pool, *id, controllers.into_inner(), queue.into_inner()).await?;

    Ok(web::Json("Delete Channel Success"))
}

/// #### ffplayout Config
///
/// **Get Advanced Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/advanced/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
#[get("/playout/advanced/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_advanced_config(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel ({id}) not exists!")))?;
    let config = manager.config.lock().await.advanced.clone();

    Ok(web::Json(config))
}

/// **Update Advanced Config**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/playout/advanced/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
#[put("/playout/advanced/{id}")]
#[protect(
    "Role::GlobalAdmin",
    "Role::ChannelAdmin",
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_advanced_config(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<AdvancedConfig>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;

    handles::update_advanced_configuration(&pool, *id, data.into_inner()).await?;
    let new_config = get_config(&pool, *id).await?;

    manager.update_config(new_config).await;

    Ok(web::Json("Update success"))
}

/// **Get Config**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/playout/config/1 -H 'Authorization: Bearer <TOKEN>'
/// ```
///
/// Response is a JSON object
#[get("/playout/config/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_playout_config(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or_else(|| ServiceError::BadRequest(format!("Channel ({id}) not exists!")))?;
    let config = manager.config.lock().await.clone();

    Ok(web::Json(config))
}

/// **Update Config**
///
/// ```BASH
/// curl -X PUT http://127.0.0.1:8787/api/playout/config/1 -H "Content-Type: application/json" \
/// -d { <CONFIG DATA> } -H 'Authorization: Bearer <TOKEN>'
/// ```
#[allow(clippy::too_many_arguments)]
#[put("/playout/config/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_playout_config(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    mut data: web::Json<PlayoutConfig>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
    mail_queues: web::Data<Mutex<Vec<Arc<Mutex<MailQueue>>>>>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let p = manager.channel.lock().await.storage.clone();
    let storage = Path::new(&p);
    let config_id = manager.config.lock().await.general.id;

    let (_, _, logo) = norm_abs_path(storage, &data.processing.logo)?;
    let (_, _, filler) = norm_abs_path(storage, &data.storage.filler)?;
    let (_, _, font) = norm_abs_path(storage, &data.text.font)?;

    data.processing.logo = logo;
    data.storage.filler = filler;
    data.text.font = font;

    handles::update_configuration(&pool, config_id, data.into_inner()).await?;
    let new_config = get_config(&pool, *id).await?;
    let mut queues = mail_queues.lock().await;

    for queue in queues.iter_mut() {
        let mut queue_lock = queue.lock().await;

        if queue_lock.id == *id {
            if queue_lock.config.recipient != new_config.mail.recipient {
                queue_lock.clear_raw();
            }

            queue_lock.update(new_config.mail.clone());
            break;
        }
    }

    manager.update_config(new_config).await;

    Ok(web::Json("Update success"))
}

/// #### Text Presets
///
/// Text presets are made for sending text messages to the ffplayout engine, to overlay them as a lower third.
///
/// **Get all Presets**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/presets/1 -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/presets/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_presets(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if let Ok(presets) = handles::select_presets(&pool, *id).await {
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
#[put("/presets/{channel}/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&path.0) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn update_preset(
    pool: web::Data<Pool<Sqlite>>,
    path: web::Path<(i32, i32)>,
    data: web::Json<TextPreset>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let (_, id) = path.into_inner();

    if handles::update_preset(&pool, &id, data.into_inner())
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
/// curl -X POST http://127.0.0.1:8787/api/presets/1/ -H 'Content-Type: application/json' \
/// -d '{ "name": "<PRESET NAME>", "text": "TEXT>", "x": "<X>", "y": "<Y>", "fontsize": 24, "line_spacing": 4, "fontcolor": "#ffffff", "box": 1, "boxcolor": "#000000", "boxborderw": 4, "alpha": 1.0, "channel_id": 1 }' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/presets/{id}/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn add_preset(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    data: web::Json<TextPreset>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    if handles::insert_preset(&pool, data.into_inner())
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
/// curl -X DELETE http://127.0.0.1:8787/api/presets/1/1 -H 'Content-Type: application/json' \
/// -H 'Authorization: Bearer <TOKEN>'
/// ```
#[delete("/presets/{channel}/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&path.0) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn delete_preset(
    pool: web::Data<Pool<Sqlite>>,
    path: web::Path<(i32, i32)>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let (_, id) = path.into_inner();

    if handles::delete_preset(&pool, &id).await.is_ok() {
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn send_text_message(
    id: web::Path<i32>,
    data: web::Json<TextFilter>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;

    match send_message(manager, data.into_inner()).await {
        Ok(res) => Ok(web::Json(res)),
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn control_playout(
    pool: web::Data<Pool<Sqlite>>,
    id: web::Path<i32>,
    control: web::Json<ControlParams>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;

    if manager.is_processing.load(Ordering::SeqCst) {
        return Err(ServiceError::Conflict(
            "A command is already being processed, please wait".to_string(),
        ));
    }

    manager.is_processing.store(true, Ordering::SeqCst);

    let resp = match control_state(&pool, &manager, &control.control).await {
        Ok(res) => Ok(web::Json(res)),
        Err(e) => Err(e),
    };

    manager.is_processing.store(false, Ordering::SeqCst);

    resp
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
///     {
///       "media": {
///         "category": "",
///         "duration": 154.2,
///         "out": 154.2,
///         "in": 0.0,
///         "source": "/opt/tv-media/clip.mp4"
///       },
///       "index": 39,
///       "ingest": false,
///       "mode": "playlist",
///       "played": 67.808
///     }
/// ```
#[get("/control/{id}/media/current")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn media_current(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let media_map = get_data_map(&manager).await;

    Ok(web::Json(media_map))
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn process_control(
    id: web::Path<i32>,
    proc: web::Json<Process>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    manager.list_init.store(true, Ordering::SeqCst);

    if manager.is_processing.load(Ordering::SeqCst) {
        return Err(ServiceError::Conflict(
            "A command is already being processed, please wait".to_string(),
        ));
    }

    manager.is_processing.store(true, Ordering::SeqCst);

    match proc.into_inner().command {
        ProcessCtl::Status => {
            manager.is_processing.store(false, Ordering::SeqCst);

            if manager.is_alive.load(Ordering::SeqCst) {
                return Ok(web::Json("active"));
            }
            return Ok(web::Json("not running"));
        }
        ProcessCtl::Start => {
            if !manager.is_alive.load(Ordering::SeqCst) {
                manager.channel.lock().await.active = true;
                manager.start().await?;
            }
        }
        ProcessCtl::Stop => {
            manager.channel.lock().await.active = false;
            manager.stop_all(true).await?;
        }
        ProcessCtl::Restart => {
            manager.channel.lock().await.active = false;
            manager.stop_all(false).await?;

            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

            manager.channel.lock().await.active = true;
            manager.start().await?;
        }
    }

    manager.is_processing.store(false, Ordering::SeqCst);

    Ok(web::Json("Success"))
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn get_playlist(
    id: web::Path<i32>,
    obj: web::Query<DateObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let config = manager.config.lock().await.clone();

    match read_playlist(&config, obj.date.clone()).await {
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn save_playlist(
    id: web::Path<i32>,
    data: web::Json<JsonPlaylist>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let config = manager.config.lock().await.clone();

    match write_playlist(&config, data.into_inner()).await {
        Ok(res) => Ok(web::Json(res)),
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&params.0) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn gen_playlist(
    params: web::Path<(i32, String)>,
    data: Option<web::Json<PathsObj>>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let (id, date) = params.into_inner();
    let manager = controllers
        .lock()
        .await
        .get(id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    manager.config.lock().await.general.generate = Some(vec![date.clone()]);
    let storage = manager.config.lock().await.channel.storage.clone();

    if let Some(obj) = data {
        if let Some(paths) = &obj.paths {
            let mut path_list = vec![];

            for path in paths {
                let (p, _, _) = norm_abs_path(&storage, path)?;

                path_list.push(p);
            }

            manager.config.lock().await.storage.paths = path_list;
        }

        manager
            .config
            .lock()
            .await
            .general
            .template
            .clone_from(&obj.template);
    }

    match generate_playlist(manager).await {
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&params.0) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn del_playlist(
    params: web::Path<(i32, String)>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let (id, date) = params.into_inner();
    let manager = controllers
        .lock()
        .await
        .get(id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let config = manager.config.lock().await.clone();

    match delete_playlist(&config, &date).await {
        Ok(m) => Ok(web::Json(m)),
        Err(e) => Err(e),
    }
}

/// ### Log file
///
/// **Read Log File**
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/log/1?date=2022-06-20
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/log/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn get_log(
    id: web::Path<i32>,
    log: web::Query<DateObj>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    read_log_file(&id, &log.date).await
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn file_browser(
    id: web::Path<i32>,
    data: web::Json<PathObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let storage = manager.storage.lock().await.clone();

    match storage.browser(&data.into_inner()).await {
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn add_dir(
    id: web::Path<i32>,
    data: web::Json<PathObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<HttpResponse, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let storage = manager.storage.lock().await;

    storage.mkdir(&data.into_inner()).await?;

    Ok(HttpResponse::Ok().into())
}

/// **Rename File**
///
/// ```BASH
/// curl -X POST http://127.0.0.1:8787/api/file/1/rename/ -H 'Content-Type: application/json'
/// -d '{"source": "<SOURCE>", "target": "<TARGET>"}' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[post("/file/{id}/rename/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn move_rename(
    id: web::Path<i32>,
    data: web::Json<MoveObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let storage = manager.storage.lock().await;

    match storage.rename(&data.into_inner()).await {
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn remove(
    id: web::Path<i32>,
    data: web::Json<PathObject>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let storage = manager.storage.lock().await;
    let recursive = data.recursive;

    match storage.remove(&data.into_inner().source, recursive).await {
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
#[allow(clippy::too_many_arguments)]
#[put("/file/{id}/upload/")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn save_file(
    id: web::Path<i32>,
    _req: HttpRequest,
    payload: Multipart,
    obj: web::Query<FileObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<HttpResponse, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let storage = manager.storage.lock().await.clone();

    // let size: u64 = req
    //     .headers()
    //     .get("content-length")
    //     .and_then(|cl| cl.to_str().ok())
    //     .and_then(|cls| cls.parse().ok())
    //     .unwrap_or(0);

    storage.upload(payload, &obj.path, false).await?;

    Ok(HttpResponse::Ok().into())
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
    req: HttpRequest,
    controllers: web::Data<Mutex<ChannelController>>,
) -> Result<actix_files::NamedFile, ServiceError> {
    let id: i32 = req.match_info().query("id").parse()?;
    let manager = controllers
        .lock()
        .await
        .get(id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let config = manager.config.lock().await;
    let storage = config.channel.storage.clone();
    let file_path = req.match_info().query("filename");
    let (path, _, _) = norm_abs_path(&storage, file_path)?;
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
/// curl -X GET http://127.0.0.1:8787/1/live/stream.m3u8
/// ```
#[get("/{id}/{public:live|preview|public}/{file_stem:.*}")]
async fn get_public(
    path: web::Path<(i32, String, String)>,
    controllers: web::Data<Mutex<ChannelController>>,
) -> Result<actix_files::NamedFile, ServiceError> {
    let (id, public, file_stem) = path.into_inner();

    let absolute_path = if file_stem.ends_with(".ts")
        || file_stem.ends_with(".m3u8")
        || file_stem.ends_with(".vtt")
    {
        let manager = controllers
            .lock()
            .await
            .get(id)
            .await
            .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
        let config = manager.config.lock().await;
        config.channel.public.join(public)
    } else {
        public_path()
    }
    .clean();

    let path = absolute_path.join(file_stem.as_str());
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn import_playlist(
    id: web::Path<i32>,
    payload: Multipart,
    obj: web::Query<ImportObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<HttpResponse, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let channel_name = manager.channel.lock().await.name.clone();
    let playlists = manager.config.lock().await.channel.playlists.clone();
    let storage = manager.storage.lock().await;
    let file = obj.file.file_name().unwrap_or_default();
    let path = env::temp_dir().join(file);
    let path_clone = path.clone();

    storage.upload(payload, &path, true).await?;

    let response = import_file(&playlists, &obj.date, Some(channel_name), &path_clone).await?;

    fs::remove_file(path).await?;

    Ok(HttpResponse::Ok().body(response))
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
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
async fn get_program(
    id: web::Path<i32>,
    obj: web::Query<ProgramObj>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let config = manager.config.lock().await.clone();
    let id = config.general.channel_id;
    let start_sec = config.playlist.start_sec.unwrap();
    let mut days = 0;
    let mut program = vec![];
    let after = obj.start_after;
    let mut before = obj.start_before;

    if after > before {
        before = chrono::Local
            .with_ymd_and_hms(after.year(), after.month(), after.day(), 23, 59, 59)
            .unwrap()
            .naive_local();
    }

    if start_sec
        > time_to_sec(
            &after.format("%H:%M:%S").to_string(),
            &config.channel.timezone,
        )
    {
        days = 1;
    }

    let date_range = get_date_range(
        id,
        &vec_strings![
            (after - TimeDelta::try_days(days).unwrap_or_default()).format("%Y-%m-%d"),
            "-",
            before.format("%Y-%m-%d")
        ],
    );

    for date in date_range {
        let mut naive = NaiveDateTime::parse_from_str(
            &format!("{date} {}", sec_to_time(start_sec)),
            "%Y-%m-%d %H:%M:%S%.3f",
        )
        .unwrap();

        let playlist = match read_playlist(&config, date.clone()).await {
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
                title: item.title,
                r#in: item.seek,
                out: item.out,
                duration: item.duration,
                category: item.category,
            };

            if naive >= after && naive <= before {
                program.push(p_item);
            }

            naive += TimeDelta::try_milliseconds(((item.out - item.seek) * 1000.0) as i64)
                .unwrap_or_default();
        }
    }

    Ok(web::Json(program))
}

/// ### System Statistics
///
/// Get statistics about CPU, Ram, Disk, etc. usage.
///
/// ```BASH
/// curl -X GET http://127.0.0.1:8787/api/system/1
/// -H 'Content-Type: application/json' -H 'Authorization: Bearer <TOKEN>'
/// ```
#[get("/system/{id}")]
#[protect(
    any("Role::GlobalAdmin", "Role::ChannelAdmin", "Role::User"),
    ty = "Role",
    expr = "user.channels.contains(&*id) || role.has_authority(&Role::GlobalAdmin)"
)]
pub async fn get_system_stat(
    id: web::Path<i32>,
    controllers: web::Data<Mutex<ChannelController>>,
    role: AuthDetails<Role>,
    user: web::ReqData<UserMeta>,
) -> Result<impl Responder, ServiceError> {
    let manager = controllers
        .lock()
        .await
        .get(*id)
        .await
        .ok_or(ServiceError::BadRequest("Channel not found".to_string()))?;
    let config = manager.config.lock().await.clone();

    let stat = web::block(move || system::stat(&config)).await?;

    Ok(web::Json(stat))
}
