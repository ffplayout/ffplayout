use crate::api::{
    handles::{add_user, db_connection},
    models::User,
};
use actix_web::{get, post, web, Responder};
use sha_crypt::{sha512_simple, Sha512Params};

#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    format!("Hello {name}!")
}

/// curl -X POST -H "Content-Type: application/json" -d '{"username": "USER", "password": "abc123", "email":"user@example.org" }' http://127.0.0.1:8080/api/user/
#[post("/api/user/")]
pub async fn user(user: web::Json<User>) -> impl Responder {
    let params = Sha512Params::new(10_000).expect("RandomError!");

    let hashed_password = sha512_simple(&user.password, &params).expect("Should not fail");

    // // Verifying a stored password
    // assert!(sha512_check("Not so secure password", &hashed_password).is_ok());

    if let Ok(pool) = db_connection().await {
        if let Err(e) = add_user(
            &pool,
            &user.email,
            &user.username,
            &hashed_password,
            &user.group_id,
        )
        .await
        {
            pool.close().await;
            return e.to_string();
        };

        pool.close().await;
    }

    format!("User {} added", user.username)
}
