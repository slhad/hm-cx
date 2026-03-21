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

// Serve /data.json. If a live snapshot was injected we consider it authoritative:
// - If the snapshot contains data, return it as application/json (200).
// - If the snapshot is present but empty or null, treat this as a server-side error
//   (mappings produced no values) and return HTTP 500.
// If no snapshot was injected (no config.yml / sampler), fall back to the bundled asset.
pub async fn handle_data_json(
    data: Option<web::Data<std::sync::Arc<std::sync::RwLock<Value>>>>,
) -> impl Responder {
    log_request("GET /data.json");

    if let Some(arc_data) = data {
        let arc = arc_data.get_ref();
        let r = arc.read().unwrap();
        // If snapshot is null or empty object, mappings produced no value -> 500
        if r.is_null() || (r.is_object() && r.as_object().unwrap().is_empty()) {
            // provide more context in the error payload to aid debugging/clients
            let snapshot_size = if r.is_null() {
                0usize
            } else {
                r.as_object().map(|o| o.len()).unwrap_or(0)
            };
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let body = serde_json::json!({
                "error": "no sensor data available",
                "reason": "mappings produced no values",
                "snapshot_size": snapshot_size,
                "timestamp_unix": ts,
                "suggestion": "inspect /snapshot.json and verify config.yml mappings"
            });

            return HttpResponse::InternalServerError()
                .content_type("application/json; charset=utf-8")
                .body(body.to_string());
        }

        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(r.to_string());
    }

    // no live snapshot injected: return the bundled asset for compatibility
    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(OPEN_HARDWARE_MONITOR_DATA_JSON)
}

// Debug endpoint: return the raw in-memory snapshot regardless of emptiness.
pub async fn handle_snapshot_json(
    data: Option<web::Data<std::sync::Arc<std::sync::RwLock<Value>>>>,
) -> impl Responder {
    log_request("GET /snapshot.json");

    if let Some(arc_data) = data {
        let arc = arc_data.get_ref();
        let r = arc.read().unwrap();
        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(r.to_string());
    }

    HttpResponse::NotFound().body("no snapshot available")
}

// Health endpoint: returns 200 when snapshot is present and non-empty, 500 otherwise.
pub async fn handle_health(
    data: Option<web::Data<std::sync::Arc<std::sync::RwLock<Value>>>>,
) -> impl Responder {
    log_request("GET /health");

    if let Some(arc_data) = data {
        let arc = arc_data.get_ref();
        let r = arc.read().unwrap();
        if r.is_null() || (r.is_object() && r.as_object().unwrap().is_empty()) {
            return HttpResponse::InternalServerError()
                .content_type("application/json; charset=utf-8")
                .body(
                    serde_json::json!({"status":"unhealthy","reason":"no snapshot data"})
                        .to_string(),
                );
        }

        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(
                serde_json::json!({"status":"ok","snapshot_size": r.as_object().unwrap().len()})
                    .to_string(),
            );
    }

    // No snapshot configured: treat as healthy because server will serve the bundled asset
    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(serde_json::json!({"status":"ok","snapshot_size":0,"note":"no live snapshot configured"}).to_string())
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
