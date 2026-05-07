use axum::{
    Router,
    routing::{delete, get, post, put},
};

use crate::{
    api::{auth, routes::*, state::AppState},
    sse,
};

fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/login", post(auth::login))
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
                .route("/control/{id}/text", post(send_text_message))
                .route("/control/{id}/playout", post(control_playout))
                .route("/control/{id}/media/current", get(media_current))
                .route("/control/{id}/process", post(process_control))
                .route("/file/{id}/browse", post(file_browser))
                .route("/file/{id}/create-folder", post(add_dir))
                .route("/file/{id}/rename", post(move_rename))
                .route("/file/{id}/remove", post(remove))
                .route("/file/{id}/upload", put(upload_file))
                .route("/file/{id}/import", put(import_playlist))
                .route("/log/{id}", get(get_log))
                .route("/playlist/{id}", get(get_playlist))
                .route("/playlist/{id}", post(save_playlist))
                .route("/playlist/{id}/generate/{date}", post(gen_playlist))
                .route("/playlist/{id}/{date}", delete(del_playlist))
                .route("/playout/advanced/{id}", get(get_advanced_config))
                .route(
                    "/playout/advanced/{id}/related",
                    get(get_related_advanced_config),
                )
                .route(
                    "/playout/advanced/{channel}/{id}",
                    delete(remove_related_advanced_config),
                )
                .route("/playout/advanced/{id}", put(update_advanced_config))
                .route("/playout/advanced/{id}", post(add_advanced_config))
                .route(
                    "/playout/config/{id}",
                    get(get_playout_config).put(update_playout_config),
                )
                .route("/playout/outputs/{id}", get(get_playout_outputs))
                .route("/presets/{id}", get(get_presets))
                .route(
                    "/presets/{channel}/{id}",
                    put(update_preset).delete(delete_preset),
                )
                .route("/presets/{id}", post(add_preset))
                .route("/program/{id}", get(get_program))
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
