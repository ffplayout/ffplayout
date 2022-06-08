use actix_web::{get, post, web, Responder};
use argon2::{password_hash::PasswordHash, Argon2, PasswordVerifier};
use serde::Serialize;
use simplelog::*;

use crate::api::{handles::get_login, models::User};

#[get("/hello/{name}")]
async fn greet(name: web::Path<String>) -> impl Responder {
    format!("Hello {name}!")
}

#[derive(Serialize)]
struct ResponseObj<T> {
    message: String,
    status: i32,
    data: Option<T>,
}

/// curl -X POST -H "Content-Type: application/json" -d '{"username": "USER", "password": "abc123" }' http://127.0.0.1:8080/auth/login/
#[post("/auth/login/")]
pub async fn login(credentials: web::Json<User>) -> impl Responder {
    match get_login(&credentials.username).await {
        Ok(mut user) => {
            let pass = user.password.clone();
            user.password = "".into();
            user.salt = None;

            let hash = PasswordHash::new(&pass).unwrap();
            if Argon2::default()
                .verify_password(credentials.password.as_bytes(), &hash)
                .is_ok()
            {
                info!("user {} login", credentials.username);

                web::Json(ResponseObj {
                    message: "login correct!".into(),
                    status: 200,
                    data: Some(user),
                })
            } else {
                error!("Wrong password for {}!", credentials.username);
                web::Json(ResponseObj {
                    message: "Wrong password!".into(),
                    status: 401,
                    data: None,
                })
            }
        }
        Err(e) => {
            error!("Login {} failed! {e}", credentials.username);
            return web::Json(ResponseObj {
                message: format!("Login {} failed!", credentials.username),
                status: 404,
                data: None,
            });
        }
    }
}
