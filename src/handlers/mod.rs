use crate::mapping::generate_config;
use serde_json::Value;

use crate::utils::log_request;
use actix_web::{web, HttpResponse, Responder};
use std::collections::{HashMap, HashSet};
use std::path::Path;

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

// Serve `ohm.json` from the working directory if it exists. Returns 404 if missing
// and 500 on read errors.
pub async fn handle_ohm_json() -> impl Responder {
    log_request("GET /ohm.json");

    let path = Path::new("ohm.json");
    if !path.exists() {
        return HttpResponse::NotFound().body("ohm.json not found");
    }

    match std::fs::read_to_string(path) {
        Ok(contents) => HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(contents),
        Err(e) => {
            HttpResponse::InternalServerError().body(format!("failed to read ohm.json: {}", e))
        }
    }
}

// Compare the API's `ohm.json` (filesystem) with the served `data.json` (live snapshot or bundled asset).
// Returns JSON describing whether they are equal and a small summary of differences when not.
pub async fn handle_compare(
    data: Option<web::Data<std::sync::Arc<std::sync::RwLock<Value>>>>,
) -> impl Responder {
    log_request("GET /compare");

    let path = Path::new("ohm.json");
    if !path.exists() {
        return HttpResponse::NotFound()
            .content_type("application/json; charset=utf-8")
            .body(serde_json::json!({"error":"ohm.json not found"}).to_string());
    }

    let ohm_str = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .body(format!("failed to read ohm.json: {}", e))
        }
    };

    let ohm_value: Value = match serde_json::from_str(&ohm_str) {
        Ok(v) => v,
        Err(e) => {
            return HttpResponse::BadRequest().body(format!("failed to parse ohm.json: {}", e))
        }
    };

    let data_value: Value = if let Some(arc_data) = data {
        let arc = arc_data.get_ref();
        arc.read().unwrap().clone()
    } else {
        match serde_json::from_str(OPEN_HARDWARE_MONITOR_DATA_JSON) {
            Ok(v) => v,
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .body(format!("failed to parse bundled data.json: {}", e))
            }
        }
    };

    let equal = ohm_value == data_value;
    if equal {
        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(serde_json::json!({"equal": true}).to_string());
    }

    // If both are objects, provide a top-level key summary and differing keys
    if ohm_value.is_object() && data_value.is_object() {
        let omap = ohm_value.as_object().unwrap();
        let dmap = data_value.as_object().unwrap();
        let okeys: HashSet<String> = omap.keys().cloned().collect();
        let dkeys: HashSet<String> = dmap.keys().cloned().collect();

        let only_in_ohm: Vec<String> = okeys.difference(&dkeys).cloned().collect();
        let only_in_data: Vec<String> = dkeys.difference(&okeys).cloned().collect();

        let mut differing_keys: Vec<String> = Vec::new();
        for k in okeys.intersection(&dkeys) {
            if omap.get(k) != dmap.get(k) {
                differing_keys.push(k.clone());
            }
        }

        let body = serde_json::json!({
            "equal": false,
            "ohm_top_keys_count": omap.len(),
            "data_top_keys_count": dmap.len(),
            "only_in_ohm": only_in_ohm,
            "only_in_data": only_in_data,
            "differing_keys": differing_keys
        });

        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(body.to_string());
    }

    // Non-object values: return types and short previews
    let ohm_preview = serde_json::to_string(&ohm_value).unwrap_or_default();
    let data_preview = serde_json::to_string(&data_value).unwrap_or_default();
    let truncate = |s: String| {
        if s.len() > 1024 {
            format!("{}...[truncated]", &s[..1024])
        } else {
            s
        }
    };

    let body = serde_json::json!({
        "equal": false,
        "ohm_type": if ohm_value.is_array() { "array" } else if ohm_value.is_string() { "string" } else if ohm_value.is_number() { "number" } else if ohm_value.is_boolean() { "boolean" } else if ohm_value.is_null() { "null" } else { "object" },
        "data_type": if data_value.is_array() { "array" } else if data_value.is_string() { "string" } else if data_value.is_number() { "number" } else if data_value.is_boolean() { "boolean" } else if data_value.is_null() { "null" } else { "object" },
        "ohm_preview": truncate(ohm_preview),
        "data_preview": truncate(data_preview),
    });

    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(body.to_string())
}

// HTML view for side-by-side keys comparison. Colors:
// - blank background when key exists on both sides
// - light blue when key exists only in ohm.json
// - light green when key exists only in data.json
pub async fn handle_compare_view(
    data: Option<web::Data<std::sync::Arc<std::sync::RwLock<Value>>>>,
) -> impl Responder {
    log_request("GET /compare/view");

    let path = Path::new("ohm.json");
    if !path.exists() {
        return HttpResponse::NotFound()
            .content_type("text/html; charset=utf-8")
            .body("<h1>ohm.json not found</h1>");
    }

    let ohm_str = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .content_type("text/html; charset=utf-8")
                .body(format!("<h1>failed to read ohm.json: {}</h1>", e));
        }
    };

    let ohm_value: Value = match serde_json::from_str(&ohm_str) {
        Ok(v) => v,
        Err(e) => {
            return HttpResponse::BadRequest()
                .content_type("text/html; charset=utf-8")
                .body(format!("<h1>failed to parse ohm.json: {}</h1>", e));
        }
    };

    let data_value: Value = if let Some(arc_data) = data {
        let arc = arc_data.get_ref();
        arc.read().unwrap().clone()
    } else {
        match serde_json::from_str(OPEN_HARDWARE_MONITOR_DATA_JSON) {
            Ok(v) => v,
            Err(e) => {
                return HttpResponse::InternalServerError()
                    .content_type("text/html; charset=utf-8")
                    .body(format!("<h1>failed to parse bundled data.json: {}</h1>", e));
            }
        }
    };

    // Build a set of hierarchical paths for both sides. Paths are tokenized by '/'.
    fn collect_paths(v: &Value, base_tokens: &mut Vec<String>, out: &mut HashSet<String>) {
        match v {
            Value::Object(map) => {
                for (k, val) in map {
                    base_tokens.push(k.clone());
                    out.insert(base_tokens.join("/"));
                    if val.is_object() || val.is_array() {
                        collect_paths(val, base_tokens, out);
                    }
                    base_tokens.pop();
                }
            }
            Value::Array(arr) => {
                for (i, elem) in arr.iter().enumerate() {
                    base_tokens.push(format!("[{}]", i));
                    out.insert(base_tokens.join("/"));
                    if elem.is_object() || elem.is_array() {
                        collect_paths(elem, base_tokens, out);
                    }
                    base_tokens.pop();
                }
            }
            _ => {}
        }
    }

    // Retrieve a value at a hierarchical path like "Children/[0]/Text".
    fn get_value_at_path(v: &Value, path: &str) -> Option<Value> {
        let mut cur = v;
        if path.is_empty() {
            return Some(cur.clone());
        }
        for token in path.split('/') {
            if token.starts_with('[') && token.ends_with(']') {
                let idx = token[1..token.len() - 1].parse::<usize>().ok()?;
                match cur {
                    Value::Array(a) => cur = a.get(idx)?,
                    _ => return None,
                }
            } else {
                match cur {
                    Value::Object(m) => cur = m.get(token)?,
                    _ => return None,
                }
            }
        }
        Some(cur.clone())
    }

    fn format_value_for_display(v: &Value) -> String {
        match v {
            Value::Null => "null".to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => {
                if s.len() > 80 {
                    format!("\"{}...\"", &s[..77])
                } else {
                    format!("\"{}\"", s)
                }
            }
            Value::Array(a) => format!("[{} items]", a.len()),
            Value::Object(o) => format!("{{{} keys}}", o.len()),
        }
    }

    let mut ohm_paths: HashSet<String> = HashSet::new();
    let mut data_paths: HashSet<String> = HashSet::new();

    if ohm_value.is_object() || ohm_value.is_array() {
        collect_paths(&ohm_value, &mut Vec::new(), &mut ohm_paths);
    }
    if data_value.is_object() || data_value.is_array() {
        collect_paths(&data_value, &mut Vec::new(), &mut data_paths);
    }

    if ohm_paths.is_empty() && data_paths.is_empty() {
        return HttpResponse::Ok()
            .content_type("text/html; charset=utf-8")
            .body("<h1>No hierarchical keys found on either side</h1>");
    }

    let mut all_paths: Vec<String> = ohm_paths.union(&data_paths).cloned().collect();
    // Sort lexicographically by token sequence so parents precede children.
    all_paths.sort_by(|a, b| {
        let a_tokens: Vec<&str> = a.split('/').collect();
        let b_tokens: Vec<&str> = b.split('/').collect();
        a_tokens.cmp(&b_tokens)
    });

    // Detect case-insensitive mismatches: if the same path lowercased exists on both
    // sides but the actual-cased paths differ, mark them as case mismatches.
    let mut ohm_lower_map: HashMap<String, Vec<String>> = HashMap::new();
    for p in &ohm_paths {
        ohm_lower_map
            .entry(p.to_lowercase())
            .or_default()
            .push(p.clone());
    }
    let mut data_lower_map: HashMap<String, Vec<String>> = HashMap::new();
    for p in &data_paths {
        data_lower_map
            .entry(p.to_lowercase())
            .or_default()
            .push(p.clone());
    }

    let mut case_mismatch_paths: HashSet<String> = HashSet::new();
    let mut all_lowers: HashSet<String> = HashSet::new();
    all_lowers.extend(ohm_lower_map.keys().cloned());
    all_lowers.extend(data_lower_map.keys().cloned());
    for lower in all_lowers.iter() {
        let ovec = ohm_lower_map.get(lower);
        let dvec = data_lower_map.get(lower);
        if let (Some(ov), Some(dv)) = (ovec, dvec) {
            let oset: HashSet<String> = ov.iter().cloned().collect();
            let dset: HashSet<String> = dv.iter().cloned().collect();
            if oset != dset {
                for p in ov.iter().chain(dv.iter()) {
                    case_mismatch_paths.insert(p.clone());
                }
            }
        }
    }

    // simple HTML escape
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    };

    let mut html = String::new();
    html.push_str(r#"<!doctype html><html><head><meta charset='utf-8'><title>Compare OHM vs data.json</title><style>body{font-family:Arial,sans-serif;padding:12px}table{border-collapse:collapse;width:100%}th,td{padding:6px;border:1px solid #ddd;vertical-align:top}th{background:#f8f8f8} .ohm-only{background:#d0e7ff} .data-only{background:#d7ffd0} .case-mismatch{background:#fff3cd} .key{white-space:nowrap}</style></head><body>"#);
    html.push_str("<h1>Compare hierarchical keys: ohm.json vs data.json</h1>");
    html.push_str("<p>Blank = present in both • ");
    html.push_str(
        "<span style='background:#d0e7ff;padding:2px 6px;margin-right:6px'>OHM only</span>",
    );
    html.push_str(
        "<span style='background:#d7ffd0;padding:2px 6px;margin-right:6px'>DATA only</span>",
    );
    html.push_str(
        "<span style='background:#fff3cd;padding:2px 6px;margin-left:6px'>Case mismatch</span></p>",
    );
    html.push_str("<table><thead><tr><th style='width:50%'>Path</th><th style='width:25%'>OHM</th><th style='width:25%'>DATA</th></tr></thead><tbody>");

    for path in all_paths.iter() {
        let in_ohm = ohm_paths.contains(path);
        let in_data = data_paths.contains(path);

        let full_path_escaped = escape(path);

        // compute inline value previews
        let left_value_str = if in_ohm {
            get_value_at_path(&ohm_value, path)
                .map(|v| format_value_for_display(&v))
                .unwrap_or_default()
        } else {
            String::new()
        };
        let right_value_str = if in_data {
            get_value_at_path(&data_value, path)
                .map(|v| format_value_for_display(&v))
                .unwrap_or_default()
        } else {
            String::new()
        };

        let left_value_escaped = escape(&left_value_str);
        let right_value_escaped = escape(&right_value_str);

        // Row-level classes: color if path exists only on one side; also mark case mismatches
        let mut row_classes: Vec<&str> = Vec::new();
        if in_ohm && !in_data {
            row_classes.push("ohm-only");
        } else if in_data && !in_ohm {
            row_classes.push("data-only");
        }
        let is_case_mismatch = case_mismatch_paths.contains(path);
        if is_case_mismatch {
            row_classes.push("case-mismatch");
        }
        let row_class = row_classes.join(" ");

        let left_display = if left_value_escaped.is_empty() {
            "—".to_string()
        } else {
            left_value_escaped.clone()
        };
        let right_display = if right_value_escaped.is_empty() {
            "—".to_string()
        } else {
            right_value_escaped.clone()
        };

        let mut path_cell = format!(
            "<div style='font-family:monospace;color:#333;font-size:0.9em'>{}</div>",
            full_path_escaped
        );
        if is_case_mismatch {
            path_cell.push_str(
                "<span style='color:#b36b00;font-weight:bold;margin-left:8px'>⚠ case</span>",
            );
        }
        let ohm_cell = format!("<div style='color:#000'>{}</div>", left_display);
        let data_cell = format!("<div style='color:#000'>{}</div>", right_display);

        if row_class.is_empty() {
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                path_cell, ohm_cell, data_cell
            ));
        } else {
            html.push_str(&format!(
                "<tr class=\"{}\"><td>{}</td><td>{}</td><td>{}</td></tr>",
                row_class, path_cell, ohm_cell, data_cell
            ));
        }
    }

    html.push_str("</tbody></table></body></html>");

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
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
