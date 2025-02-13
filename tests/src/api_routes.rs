use actix_web::{get, web, App, Error, HttpResponse, Responder};

use serde_json::json;
use sqlx::{sqlite::SqlitePoolOptions, Pool, Sqlite};

use ffplayout::api::routes::login;
use ffplayout::db::{handles, init_globales, models::User};
use ffplayout::player::controller::ChannelManager;
use ffplayout::utils::config::PlayoutConfig;
// use ffplayout::validator;

async fn prepare_config() -> (PlayoutConfig, ChannelManager, Pool<Sqlite>) {
    let pool = SqlitePoolOptions::new()
        .connect("sqlite::memory:")
        .await
        .unwrap();
    handles::db_migrate(&pool).await.unwrap();

    sqlx::query(
        r#"
        UPDATE global SET public = "assets/hls", logs = "assets/log", playlists = "assets/playlists", storage = "assets/storage";
        UPDATE channels SET public = "assets/hls", playlists = "assets/playlists", storage = "assets/storage";
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    let user = User {
        id: 0,
        mail: Some("admin@mail.com".to_string()),
        username: "admin".to_string(),
        password: "admin".to_string(),
        role_id: Some(1),
        channel_ids: Some(vec![1]),
        token: None,
    };

    handles::insert_user(&pool, user.clone()).await.unwrap();

    let config = PlayoutConfig::new(&pool, 1).await.unwrap();
    let channel = handles::select_channel(&pool, &1).await.unwrap();
    let manager = ChannelManager::new(pool.clone(), channel, config.clone()).await;

    (config, manager, pool)
}

#[get("/")]
async fn get_handler() -> Result<impl Responder, Error> {
    Ok(HttpResponse::Ok())
}

#[actix_web::test]
async fn test_get() {
    let srv = actix_test::start(|| App::new().service(get_handler));

    let req = srv.get("/");
    let res = req.send().await.unwrap();

    assert!(res.status().is_success());
}

#[actix_web::test]
async fn test_login() {
    let (_, _, pool) = prepare_config().await;

    init_globales(&pool).await.unwrap();

    let srv = actix_test::start(move || {
        let db_pool = web::Data::new(pool.clone());
        App::new()
            .app_data(db_pool)
            .service(web::scope("/auth").service(login))
    });

    let payload = json!({"username": "admin", "password": "admin"});

    let res = srv.post("/auth/login/").send_json(&payload).await.unwrap();

    assert!(res.status().is_success());

    let payload = json!({"username": "admin", "password": "1234"});

    let res = srv.post("/auth/login/").send_json(&payload).await.unwrap();

    assert_eq!(res.status().as_u16(), 403);

    let payload = json!({"username": "aaa", "password": "1234"});

    let res = srv.post("/auth/login/").send_json(&payload).await.unwrap();

    assert_eq!(res.status().as_u16(), 400);
}
