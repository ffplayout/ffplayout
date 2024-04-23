/// Example for a simple auth mechanism in SSE.
///
/// get new UUID: curl -X GET http://127.0.0.1:8080/generate
/// use UUID:     curl --header "UUID: f2f8c29b-712a-48c5-8919-b535d3a05a3a" -X GET http://127.0.0.1:8080/check
///
use std::{collections::HashSet, sync::Mutex, time::Duration, time::SystemTime};

use actix_web::{middleware::Logger, web, App, HttpRequest, HttpResponse, HttpServer};
use simplelog::*;
use uuid::Uuid;

use ffplayout_lib::utils::{init_logging, PlayoutConfig};

#[derive(Debug, Eq, Hash, PartialEq)]
struct UuidData {
    uuid: Uuid,
    expiration_time: SystemTime,
}

struct AppState {
    uuids: Mutex<HashSet<UuidData>>,
}

fn prune_uuids(uuids: &mut HashSet<UuidData>) {
    uuids.retain(|entry| entry.expiration_time > SystemTime::now());
}

async fn generate_uuid(data: web::Data<AppState>) -> HttpResponse {
    let uuid = Uuid::new_v4();
    let expiration_time = SystemTime::now() + Duration::from_secs(30); // 24 * 3600 -> for 24 hours
    let mut uuids = data.uuids.lock().unwrap();

    prune_uuids(&mut uuids);

    uuids.insert(UuidData {
        uuid,
        expiration_time,
    });

    HttpResponse::Ok().body(uuid.to_string())
}

async fn check_uuid(data: web::Data<AppState>, req: HttpRequest) -> HttpResponse {
    let uuid = req.headers().get("uuid").unwrap().to_str().unwrap();
    let uuid_from_client = Uuid::parse_str(uuid).unwrap();
    let mut uuids = data.uuids.lock().unwrap();

    prune_uuids(&mut uuids);

    match uuids.iter().find(|entry| entry.uuid == uuid_from_client) {
        Some(_) => HttpResponse::Ok().body("UUID is valid"),
        None => HttpResponse::Unauthorized().body("Invalid or expired UUID"),
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let mut config = PlayoutConfig::new(None, None);
    config.mail.recipient = String::new();
    config.logging.log_to_file = false;
    config.logging.timestamp = false;

    let logging = init_logging(&config, None, None);
    CombinedLogger::init(logging).unwrap();

    let state = web::Data::new(AppState {
        uuids: Mutex::new(HashSet::new()),
    });

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .wrap(Logger::default())
            .route("/generate", web::get().to(generate_uuid))
            .route("/check", web::get().to(check_uuid))
    })
    .bind("127.0.0.1:8080")?
    .run()
    .await
}
