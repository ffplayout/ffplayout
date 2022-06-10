use actix_web::{get, http::StatusCode, post, put, web, Responder};
use actix_web_grants::proc_macro::has_permissions;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, SaltString},
    Argon2, PasswordHasher, PasswordVerifier,
};
use serde::Serialize;
use simplelog::*;

use crate::api::{
    auth::{create_jwt, Claims},
    errors::ServiceError,
    handles::{db_login, db_role, db_update_user},
    models::{LoginUser, User},
};

#[derive(Serialize)]
struct ResponseObj<T> {
    message: String,
    status: i32,
    data: Option<T>,
}

/// curl -X GET http://127.0.0.1:8080/api/settings -H "Authorization: Bearer <TOKEN>"
#[get("/settings")]
#[has_permissions("admin")]
async fn settings(user: web::ReqData<LoginUser>) -> Result<impl Responder, ServiceError> {
    println!("{:?}", user);
    Ok("Hello from settings!")
}

#[put("/user/{user_id}")]
#[has_permissions("admin")]
async fn update_user(
    user_id: web::Path<i64>,
    user: web::ReqData<LoginUser>,
    data: web::Json<User>,
) -> Result<impl Responder, ServiceError> {
    if user_id.into_inner() == user.id {
        let mut fields = String::new();

        if let Some(email) = data.email.clone() {
            fields.push_str(format!("email = '{email}'").as_str());
        }

        if !data.password.is_empty() {
            if !fields.is_empty() {
                fields.push_str(", ");
            }

            let salt = SaltString::generate(&mut OsRng);
            let argon2 = Argon2::default();

            let password_hash = argon2
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

/// curl -X POST -H "Content-Type: application/json" -d '{"username": "USER", "password": "abc123" }' \
/// http://127.0.0.1:8080/auth/login/
#[post("/auth/login/")]
pub async fn login(credentials: web::Json<User>) -> impl Responder {
    match db_login(&credentials.username).await {
        Ok(mut user) => {
            let pass = user.password.clone();
            user.password = "".into();
            user.salt = None;

            let hash = PasswordHash::new(&pass).unwrap();
            if Argon2::default()
                .verify_password(credentials.password.as_bytes(), &hash)
                .is_ok()
            {
                let role = db_role(&user.role_id.unwrap_or_default())
                    .await
                    .unwrap_or_else(|_| "guest".to_string());
                let claims = Claims::new(user.id, user.username.clone(), vec![role.clone()]);

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
                    status: 401,
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
