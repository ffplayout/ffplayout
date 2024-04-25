/// https://github.com/actix/examples/tree/master/server-sent-events
///
use std::{io, sync::Arc};

use actix_web::{get, middleware::Logger, post, web, App, HttpResponse, HttpServer, Responder};
use actix_web_lab::{extract::Path, respond::Html};

use simplelog::*;

use ffplayout_api::sse::broadcast::Broadcaster;

use ffplayout_lib::utils::{init_logging, PlayoutConfig};

#[actix_web::main]
async fn main() -> io::Result<()> {
    let mut config = PlayoutConfig::new(None, None);
    config.mail.recipient = String::new();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    let data = Broadcaster::create();

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::from(Arc::clone(&data)))
            .service(index)
            .service(event_stream)
            .service(broadcast_msg)
            .wrap(Logger::default())
    })
    .bind(("127.0.0.1", 8080))?
    .workers(2)
    .run()
    .await
}

#[get("/")]
async fn index() -> impl Responder {
    Html(include_str!("index.html").to_owned())
}

#[get("/events")]
async fn event_stream(broadcaster: web::Data<Broadcaster>) -> impl Responder {
    broadcaster
        .new_client(1, PlayoutConfig::default(), "ping".to_string())
        .await
}

#[post("/broadcast/{msg}")]
async fn broadcast_msg(
    broadcaster: web::Data<Broadcaster>,
    Path((msg,)): Path<(String,)>,
) -> impl Responder {
    broadcaster.broadcast(&msg).await;
    HttpResponse::Ok().body("msg sent")
}
