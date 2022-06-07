use crate::api::{
    handles::{add_user, db_connection, get_login, get_users},
    models::User,
};
use actix_web::{get, post, web, Responder};
use sha_crypt::{sha512_check, sha512_simple, Sha512Params};

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
            &user.email.clone().unwrap(),
            &user.username,
            &hashed_password,
            &user.group_id.unwrap(),
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

#[get("/api/user/{id}")]
pub async fn get_user(id: web::Path<i64>) -> impl Responder {
    if let Ok(pool) = db_connection().await {
        match get_users(&pool, Some(*id)).await {
            Ok(r) => {
                return web::Json(r);
            }
            Err(_) => {
                return web::Json(vec![]);
            }
        };
    }

    web::Json(vec![])
}

#[post("/auth/login/")]
pub async fn login(credentials: web::Json<User>) -> impl Responder {
    let params = Sha512Params::new(10_000).expect("RandomError!");
    let hashed_password = sha512_simple(&credentials.password, &params).expect("Should not fail");

    println!("{hashed_password}");

    if let Ok(u) = get_login(&credentials.username).await {
        println!("{}", &u[0].password);
        println!("{:?}", sha512_check(&u[0].password, &hashed_password));
        if !u.is_empty() && sha512_check(&u[0].password, &hashed_password).is_ok() {
            return "login correct!";
        }
    };

    "Login failed!"
}
