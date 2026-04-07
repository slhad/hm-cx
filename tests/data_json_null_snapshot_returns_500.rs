use actix_web::{http::header, test, web, App};
use serde_json::Value;
use std::sync::Arc;

use hm_cx::handlers::handle_data_json;
use hm_cx::mapping::LiveState;

#[actix_web::test]
async fn data_json_with_null_snapshot_returns_500() {
    let live_state = Arc::new(LiveState::new(
        serde_json::Value::Null,
        serde_json::Value::Object(serde_json::Map::new()),
    ));

    // mount handler with injected Data
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(live_state))
            .route("/data.json", web::get().to(handle_data_json)),
    )
    .await;

    let req = test::TestRequest::get().uri("/data.json").to_request();
    let resp = test::call_service(&app, req).await;

    assert_eq!(resp.status().as_u16(), 500);

    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .expect("content-type present")
        .to_str()
        .unwrap();
    assert_eq!(ct, "application/json; charset=utf-8");

    let body = test::read_body(resp).await;
    let v: Value = serde_json::from_slice(&body).expect("valid json body");
    assert_eq!(
        v.get("error").and_then(|s| s.as_str()).unwrap(),
        "no sensor data available"
    );
    // snapshot_size is 0 for null snapshot
    assert_eq!(v.get("snapshot_size").and_then(|n| n.as_u64()).unwrap(), 0);
    assert!(v.get("timestamp_unix").and_then(|n| n.as_u64()).is_some());
    assert_eq!(
        v.get("suggestion").and_then(|s| s.as_str()).unwrap(),
        "inspect /snapshot.json and verify config.yml mappings"
    );
}
