use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{delete, get, post, put},
};

use crate::{
    api::{auth, routes::*, state::AppState},
    file::MAX_UPLOAD_REQUEST_SIZE,
    sse,
};

fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/login", post(auth::login))
        .route("/logout", post(auth::logout))
        .route("/refresh", post(auth::refresh))
        .route("/verify", post(auth::verify))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .nest("/auth", auth_routes())
        .nest(
            "/api",
            Router::new()
                .merge(sse::routes::api_routes())
                .route("/channel/{id}", get(get_channel).patch(patch_channel))
                .route("/channel", post(add_channel))
                .route("/channel/{id}", delete(remove_channel))
                .route("/channels", get(get_all_channels))
                .route("/global", get(get_global).put(update_global))
                .route("/control/{id}/text", post(send_text_message))
                .route("/control/{id}/playout", post(control_playout))
                .route("/control/{id}/audio", put(update_audio_effects))
                .route("/control/{id}/media/current", get(media_current))
                .route("/control/{id}/process", post(process_control))
                .route("/file/{id}/browse", post(file_browser))
                .route("/file/{id}/create-folder", post(add_dir))
                .route("/file/{id}/rename", post(move_rename))
                .route("/file/{id}/remove", post(remove))
                .route(
                    "/file/{id}/upload",
                    get(upload_status)
                        .put(upload_file)
                        .layer(DefaultBodyLimit::max(MAX_UPLOAD_REQUEST_SIZE)),
                )
                .route("/file/{id}/import", put(import_playlist))
                .route("/file/{id}/access-token", post(create_file_access_token))
                .route("/log/{id}", get(get_log))
                .route("/playlist/{id}", get(get_playlist))
                .route("/playlist/{id}", post(save_playlist))
                .route("/playlist/{id}/generate/{date}", post(gen_playlist))
                .route("/playlist/{id}/{date}", delete(del_playlist))
                .route(
                    "/playout/config/{id}",
                    get(get_playout_config).put(update_playout_config),
                )
                .route("/playout/outputs/{id}", get(get_playout_outputs))
                .route("/playout/codecs/{id}", get(get_playout_codecs))
                .route("/text/fonts", get(get_font_families))
                .route("/presets/{id}", get(get_presets))
                .route(
                    "/presets/{channel}/{id}",
                    put(update_preset).delete(delete_preset),
                )
                .route("/presets/{id}", post(add_preset))
                .route("/program/{id}", get(get_program))
                .route("/setup", get(get_setup_status).post(complete_setup))
                .route("/system/{id}", get(get_system_stat))
                .route("/user", get(get_user))
                .route(
                    "/user/{id}",
                    get(get_by_name).put(update_user).delete(remove_user),
                )
                .route("/user", post(add_user))
                .route("/users", get(get_users)),
        )
        .nest("/data", sse::routes::data_routes())
        .route("/file/{id}/{*filename}", get(get_file))
        .route("/public/{id}/{public}/{*file_stem}", get(get_public))
}
