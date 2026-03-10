use actix_web::{HttpResponse, Responder};
use crate::utils::log_request;

const OPEN_HARDWARE_MONITOR_DATA_JSON: &str =
    include_str!("../../assets/openhardwaremonitor_localhost_8085_data.json");

pub async fn handle_index() -> impl Responder {
    log_request("GET /");
    HttpResponse::Ok().body("Welcome to the Rust Web Server!")
}

pub async fn handle_data_json() -> impl Responder {
    log_request("GET /data.json");
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