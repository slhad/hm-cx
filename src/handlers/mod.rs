use crate::mapping::generate_config;
use serde_json::Value;

use crate::utils::log_request;
use actix_web::{web, HttpResponse, Responder};

pub(crate) const OPEN_HARDWARE_MONITOR_DATA_JSON: &str =
    include_str!("../../assets/openhardwaremonitor_localhost_8085_data.json");

pub async fn handle_index() -> impl Responder {
    log_request("GET /");
    HttpResponse::Ok().body("Welcome to the Rust Web Server!")
}

pub async fn handle_generate_config() -> impl Responder {
    log_request("POST /generate-config");
    match generate_config() {
        Ok(()) => HttpResponse::Ok().body("config.yml generated"),
        Err(e) => HttpResponse::InternalServerError().body(format!("failed: {}", e)),
    }
}

// Serve /data.json. Prefer the injected live snapshot when present and non-empty; otherwise fall back to bundled asset.
pub async fn handle_data_json(
    data: Option<web::Data<std::sync::Arc<std::sync::RwLock<Value>>>>,
) -> impl Responder {
    log_request("GET /data.json");

    if let Some(arc_data) = data {
        let arc = arc_data.get_ref();
        let r = arc.read().unwrap();
        // prefer snapshot when not null and not empty object
        if !(r.is_null() || (r.is_object() && r.as_object().unwrap().is_empty())) {
            return HttpResponse::Ok()
                .content_type("application/json; charset=utf-8")
                .body(r.to_string());
        }
    }

    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(OPEN_HARDWARE_MONITOR_DATA_JSON)
}

// Additional handlers can be added here

#[cfg(test)]
mod tests {
    use super::OPEN_HARDWARE_MONITOR_DATA_JSON;
    use crate::routes::init_routes;
    use actix_web::{http::header, test, App};

    #[actix_web::test]
    async fn data_json_route_returns_the_bundled_asset() {
        let app = test::init_service(App::new().configure(init_routes)).await;
        let request = test::TestRequest::get().uri("/data.json").to_request();
        let response = test::call_service(&app, request).await;

        assert!(response.status().is_success());

        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content type header should be present")
            .to_str()
            .expect("content type header should be valid UTF-8");
        assert_eq!(content_type, "application/json; charset=utf-8");

        let body = test::read_body(response).await;
        assert_eq!(body.as_ref(), OPEN_HARDWARE_MONITOR_DATA_JSON.as_bytes());
    }
}
