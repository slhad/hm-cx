use crate::mapping::{generate_config, load_bundled_raw_data, load_config_from_file, LiveState};
use serde_json::Value;

use crate::utils::log_request;
use actix_web::{web, HttpResponse, Responder};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;

pub(crate) const OPEN_HARDWARE_MONITOR_DATA_JSON: &str =
    include_str!("../../assets/openhardwaremonitor_localhost_8085_data.json");

pub async fn handle_index() -> impl Responder {
    log_request("GET /");
    HttpResponse::Ok().body("Welcome to the Rust Web Server!")
}

pub async fn handle_live() -> impl Responder {
    log_request("GET /live");

    let html = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Live Sensor View</title>
  <style>
    :root {
      --bg: #07111a;
      --panel: rgba(10, 23, 35, 0.78);
      --panel-strong: rgba(14, 32, 47, 0.92);
      --line: rgba(143, 198, 255, 0.18);
      --text: #eef7ff;
      --muted: #8ea9bf;
      --accent: #8fd5ff;
      --accent-strong: #41e0c3;
      --warn: #ffad70;
      --danger: #ff7d89;
      --shadow: 0 24px 80px rgba(0, 0, 0, 0.38);
    }

    * { box-sizing: border-box; }
    body {
      margin: 0;
      min-height: 100vh;
      font-family: "Trebuchet MS", "Gill Sans", sans-serif;
      color: var(--text);
      background:
        radial-gradient(circle at 18% 18%, rgba(65, 224, 195, 0.18), transparent 28%),
        radial-gradient(circle at 82% 14%, rgba(143, 213, 255, 0.2), transparent 24%),
        radial-gradient(circle at 50% 100%, rgba(255, 173, 112, 0.14), transparent 34%),
        linear-gradient(155deg, #041019 0%, #07111a 38%, #0c1721 100%);
      overflow-x: hidden;
    }

    body::before {
      content: "";
      position: fixed;
      inset: 0;
      pointer-events: none;
      background-image:
        linear-gradient(rgba(255,255,255,0.03) 1px, transparent 1px),
        linear-gradient(90deg, rgba(255,255,255,0.03) 1px, transparent 1px);
      background-size: 72px 72px;
      mask-image: radial-gradient(circle at center, black 42%, transparent 88%);
      opacity: 0.35;
    }

    main {
      width: min(1200px, calc(100% - 32px));
      margin: 0 auto;
      padding: 32px 0 56px;
      position: relative;
      z-index: 1;
    }

    .hero {
      position: relative;
      overflow: hidden;
      border: 1px solid var(--line);
      border-radius: 28px;
      padding: 28px;
      background:
        linear-gradient(135deg, rgba(12, 28, 40, 0.96), rgba(5, 16, 26, 0.78)),
        var(--panel);
      box-shadow: var(--shadow);
    }

    .hero::after {
      content: "";
      position: absolute;
      inset: auto -8% -25% auto;
      width: 280px;
      height: 280px;
      border-radius: 50%;
      background: radial-gradient(circle, rgba(143, 213, 255, 0.28), transparent 64%);
      filter: blur(10px);
    }

    .eyebrow {
      display: inline-flex;
      align-items: center;
      gap: 10px;
      padding: 8px 12px;
      border-radius: 999px;
      font-size: 12px;
      letter-spacing: 0.2em;
      text-transform: uppercase;
      color: var(--accent);
      background: rgba(143, 213, 255, 0.1);
      border: 1px solid rgba(143, 213, 255, 0.18);
    }

    h1 {
      margin: 18px 0 10px;
      max-width: 10ch;
      font-family: "Iowan Old Style", "Palatino Linotype", serif;
      font-size: clamp(2.8rem, 6vw, 5.4rem);
      line-height: 0.92;
      letter-spacing: -0.04em;
      font-weight: 600;
    }

    .subtitle {
      max-width: 60ch;
      margin: 0;
      color: var(--muted);
      font-size: 1.02rem;
      line-height: 1.6;
    }

    .hero-grid,
    .sensor-grid {
      display: grid;
      gap: 16px;
      margin-top: 24px;
    }

    .hero-grid {
      grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    }

    .sensor-grid {
      grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
      margin-top: 18px;
    }

    .panel,
    .sensor-card {
      position: relative;
      overflow: hidden;
      border: 1px solid var(--line);
      border-radius: 22px;
      background: var(--panel);
      backdrop-filter: blur(10px);
      box-shadow: var(--shadow);
    }

    .panel { padding: 18px; }
    .sensor-card { padding: 18px; min-height: 174px; }

    .panel::before,
    .sensor-card::before {
      content: "";
      position: absolute;
      inset: 0;
      background: linear-gradient(180deg, rgba(255,255,255,0.06), transparent 35%);
      pointer-events: none;
    }

    .kicker {
      font-size: 0.72rem;
      text-transform: uppercase;
      letter-spacing: 0.18em;
      color: var(--muted);
      margin-bottom: 12px;
    }

    .metric {
      display: flex;
      align-items: baseline;
      justify-content: space-between;
      gap: 12px;
    }

    .metric strong {
      font-size: clamp(1.5rem, 2vw, 2.1rem);
      font-weight: 600;
    }

    .status-pill {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      border-radius: 999px;
      font-size: 0.82rem;
      background: rgba(255,255,255,0.06);
      color: var(--text);
    }

    .status-pill::before {
      content: "";
      width: 9px;
      height: 9px;
      border-radius: 50%;
      background: currentColor;
      box-shadow: 0 0 18px currentColor;
    }

    .ok { color: var(--accent-strong); }
    .warn { color: var(--warn); }
    .bad { color: var(--danger); }

    .section-head {
      margin-top: 34px;
      display: flex;
      align-items: end;
      justify-content: space-between;
      gap: 12px;
    }

    .section-head h2 {
      margin: 0;
      font-family: "Iowan Old Style", "Palatino Linotype", serif;
      font-size: clamp(1.6rem, 3vw, 2.4rem);
      letter-spacing: -0.03em;
      font-weight: 600;
    }

    .section-head p,
    .sensor-meta,
    .metric-note,
    .footer-note {
      margin: 0;
      color: var(--muted);
    }

    .sensor-type {
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 6px 10px;
      border-radius: 999px;
      background: rgba(143, 213, 255, 0.08);
      color: var(--accent);
      font-size: 0.74rem;
      text-transform: uppercase;
      letter-spacing: 0.14em;
    }

    .sensor-value {
      margin: 18px 0 12px;
      font-size: clamp(1.55rem, 2vw, 2.2rem);
      line-height: 1;
      font-weight: 600;
      letter-spacing: -0.03em;
    }

    .sensor-card:nth-child(3n+1) {
      background: linear-gradient(180deg, rgba(143, 213, 255, 0.12), rgba(8, 20, 31, 0.88));
    }

    .sensor-card:nth-child(3n+2) {
      background: linear-gradient(180deg, rgba(65, 224, 195, 0.12), rgba(8, 20, 31, 0.88));
    }

    .sensor-card:nth-child(3n+3) {
      background: linear-gradient(180deg, rgba(255, 173, 112, 0.12), rgba(8, 20, 31, 0.88));
    }

    .empty {
      padding: 22px;
      border-radius: 22px;
      border: 1px dashed rgba(143, 213, 255, 0.2);
      color: var(--muted);
      background: rgba(9, 19, 29, 0.62);
    }

    .footer-note {
      margin-top: 24px;
      font-size: 0.88rem;
    }

    @media (max-width: 720px) {
      main { width: min(100% - 20px, 1200px); padding-top: 20px; }
      .hero, .panel, .sensor-card { border-radius: 20px; }
      h1 { max-width: none; }
      .section-head { align-items: start; flex-direction: column; }
    }
  </style>
</head>
<body>
  <main>
    <section class="hero">
      <span class="eyebrow">Live Telemetry Theatre</span>
      <h1>Machine pulse, without the tree noise.</h1>
      <p class="subtitle">
        A focused live view over the OHM-shaped data stream. The page polls the server,
        highlights health and schema compatibility, and surfaces the sensors that move the most eyes.
      </p>
      <div class="hero-grid">
        <article class="panel">
          <div class="kicker">Server Health</div>
          <div class="metric">
            <strong id="health-value">Waiting</strong>
            <span class="status-pill warn" id="health-pill">No sample yet</span>
          </div>
          <p class="metric-note" id="health-note">Polling <code>/health</code> and <code>/compare/schema</code>.</p>
        </article>
        <article class="panel">
          <div class="kicker">Schema Lock</div>
          <div class="metric">
            <strong id="schema-value">Pending</strong>
            <span class="status-pill warn" id="schema-pill">Checking</span>
          </div>
          <p class="metric-note" id="schema-note">Value drift is ignored. Shape and units must hold.</p>
        </article>
        <article class="panel">
          <div class="kicker">Last Refresh</div>
          <div class="metric">
            <strong id="refresh-value">-</strong>
            <span class="status-pill ok" id="refresh-pill">Auto 5s</span>
          </div>
          <p class="metric-note" id="refresh-note">No payload received yet.</p>
        </article>
      </div>
    </section>

    <div class="section-head">
      <div>
        <h2>Spotlight Sensors</h2>
        <p>Curated from the live OHM tree by signal quality and recognisable labels.</p>
      </div>
      <p id="summary-text">Fetching live data...</p>
    </div>
    <section class="sensor-grid" id="sensor-grid">
      <div class="empty">Waiting for the first live payload from <code>/data.json</code>.</div>
    </section>

    <p class="footer-note">
      This view reads directly from the existing server endpoints. No extra API surface is required for the dashboard.
    </p>
  </main>

  <script>
    const sensorGrid = document.getElementById("sensor-grid");
    const summaryText = document.getElementById("summary-text");
    const healthValue = document.getElementById("health-value");
    const healthPill = document.getElementById("health-pill");
    const healthNote = document.getElementById("health-note");
    const schemaValue = document.getElementById("schema-value");
    const schemaPill = document.getElementById("schema-pill");
    const schemaNote = document.getElementById("schema-note");
    const refreshValue = document.getElementById("refresh-value");
    const refreshNote = document.getElementById("refresh-note");

    const preferredSensors = [
      /cpu/i,
      /gpu/i,
      /package/i,
      /core/i,
      /fan/i,
      /temperature/i,
      /power/i,
      /clock/i,
      /memory/i,
      /pump/i
    ];

    function setPill(node, text, className) {
      node.textContent = text;
      node.className = "status-pill " + className;
    }

    function flattenSensors(node, path = []) {
      const sensors = [];
      if (!node || typeof node !== "object") return sensors;

      const nextPath = node.Text ? path.concat(node.Text) : path;
      if (node.SensorId && typeof node.Value === "string" && node.Value.trim()) {
        sensors.push({
          id: node.SensorId,
          text: node.Text || "Unnamed sensor",
          type: node.Type || "Unknown",
          value: node.Value,
          min: node.Min || "-",
          max: node.Max || "-",
          path: nextPath.join(" / ")
        });
      }

      if (Array.isArray(node.Children)) {
        for (const child of node.Children) {
          sensors.push(...flattenSensors(child, nextPath));
        }
      }
      return sensors;
    }

    function sensorScore(sensor) {
      const label = (sensor.text + " " + sensor.type).toLowerCase();
      let score = 0;
      preferredSensors.forEach((pattern, index) => {
        if (pattern.test(label)) score += 30 - index;
      });
      if (sensor.value.includes("°C")) score += 10;
      if (sensor.value.includes("RPM")) score += 8;
      if (sensor.value.includes("W")) score += 7;
      if (sensor.value.includes("%")) score += 6;
      return score;
    }

    function renderSensors(sensors) {
      if (!sensors.length) {
        sensorGrid.innerHTML = '<div class="empty">No sensor cards could be extracted from the current payload.</div>';
        return;
      }

      const selected = sensors
        .filter(sensor => sensor.value !== "-" && sensor.value.trim() !== "")
        .sort((a, b) => sensorScore(b) - sensorScore(a) || a.text.localeCompare(b.text))
        .slice(0, 12);

      sensorGrid.innerHTML = selected.map(sensor => `
        <article class="sensor-card">
          <span class="sensor-type">${sensor.type}</span>
          <div class="sensor-value">${sensor.value}</div>
          <div class="sensor-meta">${sensor.text}</div>
          <p class="metric-note" style="margin-top:12px;">Min ${sensor.min}</p>
          <p class="metric-note">Max ${sensor.max}</p>
          <p class="metric-note" style="margin-top:12px;">${sensor.path}</p>
        </article>
      `).join("");
    }

    async function loadHealth() {
      try {
        const response = await fetch("/health", { cache: "no-store" });
        const payload = await response.json();
        healthValue.textContent = payload.status === "ok" ? "Healthy" : "Unhealthy";
        setPill(healthPill, payload.status === "ok" ? "Live snapshot ready" : "Snapshot issue", payload.status === "ok" ? "ok" : "bad");
        healthNote.textContent = payload.note || `Snapshot size ${payload.snapshot_size ?? 0}`;
      } catch (error) {
        healthValue.textContent = "Offline";
        setPill(healthPill, "Health fetch failed", "bad");
        healthNote.textContent = error.message;
      }
    }

    async function loadSchema() {
      try {
        const response = await fetch("/compare/schema", { cache: "no-store" });
        const payload = await response.json();
        schemaValue.textContent = payload.compatible ? "Compatible" : "Mismatch";
        setPill(schemaPill, payload.compatible ? "Shape locked" : "Needs attention", payload.compatible ? "ok" : "warn");
        if (payload.compatible) {
          schemaNote.textContent = "No path, metadata, or unit-family mismatches detected.";
        } else {
          schemaNote.textContent = `${payload.unit_mismatches.length} unit mismatches, ${payload.metadata_mismatches.length} metadata mismatches.`;
        }
      } catch (error) {
        schemaValue.textContent = "Unknown";
        setPill(schemaPill, "Schema fetch failed", "bad");
        schemaNote.textContent = error.message;
      }
    }

    async function loadData() {
      try {
        const response = await fetch("/data.json", { cache: "no-store" });
        const payload = await response.json();
        const sensors = flattenSensors(payload);
        renderSensors(sensors);
        summaryText.textContent = `${sensors.length} live sensors discovered in the OHM tree.`;
        const now = new Date();
        refreshValue.textContent = now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
        refreshNote.textContent = `Latest pull completed at ${now.toLocaleString()}.`;
      } catch (error) {
        summaryText.textContent = "Live data fetch failed.";
        sensorGrid.innerHTML = `<div class="empty">${error.message}</div>`;
        refreshValue.textContent = "Error";
        refreshNote.textContent = error.message;
      }
    }

    async function refreshAll() {
      await Promise.all([loadHealth(), loadSchema(), loadData()]);
    }

    refreshAll();
    setInterval(refreshAll, 5000);
  </script>
</body>
</html>"#;

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
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
pub async fn handle_data_json(data: Option<web::Data<Arc<LiveState>>>) -> impl Responder {
    log_request("GET /data.json");

    if let Some(state) = live_state_from_data(data) {
        let r = state.rendered.read().unwrap();
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
pub async fn handle_snapshot_json(data: Option<web::Data<Arc<LiveState>>>) -> impl Responder {
    log_request("GET /snapshot.json");

    if let Some(state) = live_state_from_data(data) {
        let r = state.raw.read().unwrap();
        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(r.to_string());
    }

    HttpResponse::NotFound().body("no snapshot available")
}

// Explicit raw endpoint: return the raw SensorId map without OHM tree wrapping.
pub async fn handle_raw_data_json(data: Option<web::Data<Arc<LiveState>>>) -> impl Responder {
    log_request("GET /rawData.json");

    if let Some(state) = live_state_from_data(data) {
        let r = state.raw.read().unwrap();
        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(r.to_string());
    }

    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(load_bundled_raw_data().to_string())
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

fn load_ohm_value() -> Result<Value, HttpResponse> {
    let path = Path::new("ohm.json");
    if !path.exists() {
        return Err(HttpResponse::NotFound()
            .content_type("application/json; charset=utf-8")
            .body(serde_json::json!({"error":"ohm.json not found"}).to_string()));
    }

    let ohm_str = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            return Err(
                HttpResponse::InternalServerError().body(format!("failed to read ohm.json: {}", e))
            );
        }
    };

    match serde_json::from_str(&ohm_str) {
        Ok(v) => Ok(v),
        Err(e) => Err(HttpResponse::BadRequest().body(format!("failed to parse ohm.json: {}", e))),
    }
}

fn collect_raw_sensor_entries(v: &Value, out: &mut serde_json::Map<String, Value>) {
    match v {
        Value::Object(map) => {
            if let Some(sensor_id) = map.get("SensorId").and_then(|s| s.as_str()) {
                let mut entry = serde_json::Map::new();
                for key in [
                    "Text", "Type", "Value", "RawValue", "Min", "Max", "RawMin", "RawMax",
                ] {
                    if let Some(value) = map.get(key) {
                        entry.insert(key.to_string(), value.clone());
                    }
                }
                out.insert(sensor_id.to_string(), Value::Object(entry));
            }

            if let Some(children) = map.get("Children").and_then(|children| children.as_array()) {
                for child in children {
                    collect_raw_sensor_entries(child, out);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                collect_raw_sensor_entries(item, out);
            }
        }
        _ => {}
    }
}

fn load_ohm_raw_value() -> Result<Value, HttpResponse> {
    let ohm_value = load_ohm_value()?;
    let mut raw = serde_json::Map::new();
    collect_raw_sensor_entries(&ohm_value, &mut raw);
    Ok(Value::Object(raw))
}

fn current_rendered_data(data: Option<web::Data<Arc<LiveState>>>) -> Value {
    if let Some(state) = live_state_from_data(data) {
        return state.rendered.read().unwrap().clone();
    }

    serde_json::from_str(OPEN_HARDWARE_MONITOR_DATA_JSON).unwrap_or(Value::Null)
}

fn current_raw_data(data: Option<web::Data<Arc<LiveState>>>) -> Value {
    if let Some(state) = live_state_from_data(data) {
        return state.raw.read().unwrap().clone();
    }

    load_bundled_raw_data()
}

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

fn normalize_unit(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return trimmed.to_string();
    }

    trimmed.split_whitespace().nth(1).unwrap_or("").to_string()
}

fn unit_family(unit: &str) -> &str {
    if unit.ends_with("B/s") {
        return "bytes_per_second";
    }
    if unit.ends_with('B') {
        return "bytes";
    }
    unit
}

fn units_are_compatible(data_unit: &str, ohm_unit: &str) -> bool {
    if data_unit == ohm_unit {
        return true;
    }

    // A dash in the OHM snapshot means no value was available at capture time.
    if ohm_unit == "-" {
        return true;
    }

    unit_family(data_unit) == unit_family(ohm_unit)
}

fn collect_field_signatures(
    v: &Value,
    base_tokens: &mut Vec<String>,
    unit_fields: &mut BTreeMap<String, String>,
    metadata_fields: &mut BTreeMap<String, String>,
) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                base_tokens.push(k.clone());
                let path = base_tokens.join("/");
                match (k.as_str(), val) {
                    ("Value" | "Min" | "Max", Value::String(s)) => {
                        unit_fields.insert(path, normalize_unit(s));
                    }
                    ("SensorId" | "Type" | "Text", Value::String(s)) => {
                        metadata_fields.insert(path, s.clone());
                    }
                    _ => {}
                }

                if val.is_object() || val.is_array() {
                    collect_field_signatures(val, base_tokens, unit_fields, metadata_fields);
                }
                base_tokens.pop();
            }
        }
        Value::Array(arr) => {
            for (i, elem) in arr.iter().enumerate() {
                base_tokens.push(format!("[{}]", i));
                if elem.is_object() || elem.is_array() {
                    collect_field_signatures(elem, base_tokens, unit_fields, metadata_fields);
                }
                base_tokens.pop();
            }
        }
        _ => {}
    }
}

fn compare_schema(data_value: &Value, ohm_value: &Value) -> Value {
    let mut data_paths = HashSet::new();
    let mut ohm_paths = HashSet::new();
    collect_paths(data_value, &mut Vec::new(), &mut data_paths);
    collect_paths(ohm_value, &mut Vec::new(), &mut ohm_paths);

    let only_in_data: Vec<String> = data_paths.difference(&ohm_paths).cloned().collect();
    let only_in_ohm: Vec<String> = ohm_paths.difference(&data_paths).cloned().collect();

    let mut data_units = BTreeMap::new();
    let mut ohm_units = BTreeMap::new();
    let mut data_meta = BTreeMap::new();
    let mut ohm_meta = BTreeMap::new();
    collect_field_signatures(data_value, &mut Vec::new(), &mut data_units, &mut data_meta);
    collect_field_signatures(ohm_value, &mut Vec::new(), &mut ohm_units, &mut ohm_meta);

    let mut unit_mismatches = Vec::new();
    for (path, data_unit) in &data_units {
        if let Some(ohm_unit) = ohm_units.get(path) {
            if !units_are_compatible(data_unit, ohm_unit) {
                unit_mismatches.push(serde_json::json!({
                    "path": path,
                    "data_unit": data_unit,
                    "ohm_unit": ohm_unit
                }));
            }
        }
    }

    let mut metadata_mismatches = Vec::new();
    for (path, data_value) in &data_meta {
        if let Some(ohm_value) = ohm_meta.get(path) {
            if data_value != ohm_value {
                metadata_mismatches.push(serde_json::json!({
                    "path": path,
                    "data_value": data_value,
                    "ohm_value": ohm_value
                }));
            }
        }
    }

    serde_json::json!({
        "compatible": only_in_data.is_empty()
            && only_in_ohm.is_empty()
            && unit_mismatches.is_empty()
            && metadata_mismatches.is_empty(),
        "only_in_data": only_in_data,
        "only_in_ohm": only_in_ohm,
        "unit_mismatches": unit_mismatches,
        "metadata_mismatches": metadata_mismatches,
        "note": "value drift is ignored; only schema paths, units, and sensor metadata are enforced"
    })
}

pub async fn handle_compare_schema(data: Option<web::Data<Arc<LiveState>>>) -> impl Responder {
    log_request("GET /compare/schema");

    let ohm_value = match load_ohm_value() {
        Ok(v) => v,
        Err(response) => return response,
    };
    let data_value = current_rendered_data(data);

    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(compare_schema(&data_value, &ohm_value).to_string())
}

// Compare the API's `ohm.json` (filesystem) with raw sensor data.
// Returns JSON describing whether they are equal and a small summary of differences when not.
pub async fn handle_compare(data: Option<web::Data<Arc<LiveState>>>) -> impl Responder {
    log_request("GET /compare");

    let ohm_value = match load_ohm_raw_value() {
        Ok(v) => v,
        Err(response) => return response,
    };
    let data_value = current_raw_data(data);

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
        let only_in_raw: Vec<String> = dkeys.difference(&okeys).cloned().collect();

        let mut differing_keys: Vec<String> = Vec::new();
        for k in okeys.intersection(&dkeys) {
            if omap.get(k) != dmap.get(k) {
                differing_keys.push(k.clone());
            }
        }

        let body = serde_json::json!({
            "equal": false,
            "ohm_top_keys_count": omap.len(),
            "raw_top_keys_count": dmap.len(),
            "only_in_ohm": only_in_ohm,
            "only_in_raw": only_in_raw,
            "differing_keys": differing_keys
        });

        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(body.to_string());
    }

    // Non-object values: return types and short previews
    let ohm_preview = serde_json::to_string(&ohm_value).unwrap_or_default();
    let raw_preview = serde_json::to_string(&data_value).unwrap_or_default();
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
        "raw_type": if data_value.is_array() { "array" } else if data_value.is_string() { "string" } else if data_value.is_number() { "number" } else if data_value.is_boolean() { "boolean" } else if data_value.is_null() { "null" } else { "object" },
        "ohm_preview": truncate(ohm_preview),
        "raw_preview": truncate(raw_preview),
    });

    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(body.to_string())
}

// HTML view for side-by-side keys comparison. Colors:
// - blank background when key exists on both sides
// - light blue when key exists only in ohm.json
// - light green when key exists only in data.json
pub async fn handle_compare_view(data: Option<web::Data<Arc<LiveState>>>) -> impl Responder {
    log_request("GET /compare/view");

    let ohm_value = match load_ohm_value() {
        Ok(v) => v,
        Err(response) => return response.map_into_boxed_body(),
    };
    let data_value = current_rendered_data(data);

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
    html.push_str(r#"<!doctype html><html><head><meta charset='utf-8'><title>Compare ohm.json vs data.json</title><style>body{font-family:Arial,sans-serif;padding:12px}table{border-collapse:collapse;width:100%}th,td{padding:6px;border:1px solid #ddd;vertical-align:top}th{background:#f8f8f8} .ohm-only{background:#d0e7ff} .data-only{background:#d7ffd0} .case-mismatch{background:#fff3cd} .key{white-space:nowrap}</style></head><body>"#);
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
        let raw_cell = format!("<div style='color:#000'>{}</div>", right_display);

        if row_class.is_empty() {
            html.push_str(&format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
                path_cell, ohm_cell, raw_cell
            ));
        } else {
            html.push_str(&format!(
                "<tr class=\"{}\"><td>{}</td><td>{}</td><td>{}</td></tr>",
                row_class, path_cell, ohm_cell, raw_cell
            ));
        }
    }

    html.push_str("</tbody></table></body></html>");

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

fn collect_health_warnings() -> Vec<String> {
    let Ok(cfg) = load_config_from_file("config.yml") else {
        return Vec::new();
    };

    let mut warnings = Vec::new();
    let mut seen = HashSet::new();

    for mapping in cfg.mappings {
        let Some(path) = mapping.source.path.as_deref() else {
            continue;
        };

        let message = match mapping.source.kind.as_str() {
            "powercap_rapl" => {
                if !Path::new(path).exists() {
                    Some(format!(
                        "{} is mapped to RAPL at {} but that path does not exist on this host.",
                        mapping.text, path
                    ))
                } else {
                    match std::fs::read_to_string(path) {
                        Ok(_) => None,
                        Err(err) if err.kind() == ErrorKind::PermissionDenied => Some(format!(
                            "{} is mapped to RAPL at {} but this process cannot read it; grant cap_dac_read_search,cap_perfmon to the server binary or run with sufficient privileges.",
                            mapping.text, path
                        )),
                        Err(err) => Some(format!(
                            "{} is mapped to RAPL at {} but reads are failing: {}.",
                            mapping.text, path, err
                        )),
                    }
                }
            }
            "sysfs" => {
                if !Path::new(path).exists() {
                    Some(format!(
                        "{} is mapped to sysfs at {} but that path does not exist on this host.",
                        mapping.text, path
                    ))
                } else {
                    None
                }
            }
            _ => None,
        };

        if let Some(message) = message {
            if seen.insert(message.clone()) {
                warnings.push(message);
            }
        }
    }

    warnings
}

// Health endpoint: returns 200 when snapshot is present and non-empty, 500 otherwise.
pub async fn handle_health(data: Option<web::Data<Arc<LiveState>>>) -> impl Responder {
    log_request("GET /health");

    if let Some(state) = live_state_from_data(data) {
        let r = state.rendered.read().unwrap();
        if r.is_null() || (r.is_object() && r.as_object().unwrap().is_empty()) {
            return HttpResponse::InternalServerError()
                .content_type("application/json; charset=utf-8")
                .body(
                    serde_json::json!({"status":"unhealthy","reason":"no snapshot data"})
                        .to_string(),
                );
        }

        let warnings = collect_health_warnings();
        let status = if warnings.is_empty() {
            "ok"
        } else {
            "degraded"
        };
        let mut body = serde_json::json!({
            "status": status,
            "snapshot_size": r.as_object().unwrap().len(),
            "warnings": warnings,
        });
        if let Some(first_warning) = body["warnings"]
            .as_array()
            .and_then(|warnings| warnings.first())
        {
            body["note"] = first_warning.clone();
        }

        return HttpResponse::Ok()
            .content_type("application/json; charset=utf-8")
            .body(body.to_string());
    }

    // No snapshot configured: treat as healthy because server will serve the bundled asset
    HttpResponse::Ok()
        .content_type("application/json; charset=utf-8")
        .body(serde_json::json!({"status":"ok","snapshot_size":0,"note":"no live snapshot configured","warnings":[]}).to_string())
}

// Additional handlers can be added here

fn live_state_from_data(data: Option<web::Data<Arc<LiveState>>>) -> Option<Arc<LiveState>> {
    data.map(|arc_data| arc_data.get_ref().clone())
}

#[cfg(test)]
mod tests {
    use super::{units_are_compatible, OPEN_HARDWARE_MONITOR_DATA_JSON};
    use crate::routes::init_routes;
    use actix_web::{http::header, test, App};
    use serde_json::{json, Value};
    use std::path::PathBuf;
    use std::sync::OnceLock;
    use tempfile::tempdir;
    use tokio::sync::Mutex;

    struct CwdGuard(PathBuf);

    impl Drop for CwdGuard {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    fn cwd_mutex() -> &'static Mutex<()> {
        static CWD_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
        CWD_MUTEX.get_or_init(|| Mutex::new(()))
    }

    fn make_live_state(
        rendered: serde_json::Value,
        raw: serde_json::Value,
    ) -> std::sync::Arc<crate::mapping::LiveState> {
        std::sync::Arc::new(crate::mapping::LiveState {
            rendered: std::sync::Arc::new(std::sync::RwLock::new(rendered)),
            raw: std::sync::Arc::new(std::sync::RwLock::new(raw)),
        })
    }

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

    #[actix_web::test]
    async fn live_route_returns_html_dashboard() {
        let app = test::init_service(App::new().configure(init_routes)).await;
        let request = test::TestRequest::get().uri("/live").to_request();
        let response = test::call_service(&app, request).await;

        assert!(response.status().is_success());
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content type header should be present")
            .to_str()
            .expect("content type header should be valid UTF-8");
        assert_eq!(content_type, "text/html; charset=utf-8");

        let body = test::read_body(response).await;
        let text = String::from_utf8(body.to_vec()).expect("utf8");
        assert!(text.contains("Live Telemetry Theatre"));
        assert!(text.contains("/compare/schema"));
        assert!(text.contains("/data.json"));
    }

    #[actix_web::test]
    async fn health_route_surfaces_mapping_warnings() {
        let _cwd_lock = cwd_mutex().lock().await;
        let tempdir = tempdir().expect("tempdir");
        let _guard = CwdGuard(std::env::current_dir().expect("cwd"));
        std::env::set_current_dir(tempdir.path()).expect("set cwd");

        std::fs::write(
            tempdir.path().join("config.yml"),
            "mappings:\n  - ohm: /amdcpu/0/power/0\n    text: Package\n    type: Power\n    source:\n      kind: powercap_rapl\n      path: /definitely/missing/energy_uj\n      chip: null\n      key: null\n",
        )
        .expect("write config");

        let state = make_live_state(json!({"Children": []}), json!({}));
        let app = test::init_service(
            App::new()
                .app_data(actix_web::web::Data::new(state))
                .configure(init_routes),
        )
        .await;

        let response =
            test::call_service(&app, test::TestRequest::get().uri("/health").to_request()).await;
        assert!(response.status().is_success());
        let body = test::read_body(response).await;
        let payload: Value = serde_json::from_slice(&body).expect("health json");
        assert_eq!(
            payload.get("status").and_then(|v| v.as_str()),
            Some("degraded")
        );
        let warnings = payload
            .get("warnings")
            .and_then(|v| v.as_array())
            .expect("warnings array");
        assert!(warnings
            .iter()
            .any(|warning| warning.as_str().unwrap_or("").contains("does not exist")));
    }

    #[actix_web::test]
    async fn raw_data_json_route_returns_the_raw_snapshot() {
        let raw = json!({
            "sensor-1": {
                "Value": "42.00 °C",
                "RawValue": "42000",
                "Text": "Test Temp",
                "Type": "Temperature"
            }
        });
        let state = make_live_state(json!({"Children": []}), raw.clone());

        let app = test::init_service(
            App::new()
                .app_data(actix_web::web::Data::new(state))
                .configure(init_routes),
        )
        .await;

        let request = test::TestRequest::get().uri("/rawData.json").to_request();
        let response = test::call_service(&app, request).await;

        assert!(response.status().is_success());
        let body = test::read_body(response).await;
        assert_eq!(body.as_ref(), raw.to_string().as_bytes());
    }

    #[actix_web::test]
    async fn compare_route_uses_raw_and_view_uses_rendered_data() {
        let _cwd_lock = cwd_mutex().lock().await;
        let tempdir = tempdir().expect("tempdir");
        let orig_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(tempdir.path()).expect("set cwd");
        let _cwd_guard = CwdGuard(orig_cwd);

        let ohm = json!({
            "Children": [
                {
                    "SensorId": "sensor-1",
                    "Text": "Test Temp",
                    "Type": "Temperature",
                    "Value": "41.00 °C",
                    "RawValue": "41000",
                    "Children": []
                }
            ]
        });
        std::fs::write(
            tempdir.path().join("ohm.json"),
            serde_json::to_string(&ohm).expect("serialize ohm"),
        )
        .expect("write ohm.json");

        let raw = json!({
            "sensor-1": {
                "Value": "42.00 °C",
                "RawValue": "42000",
                "Text": "Test Temp",
                "Type": "Temperature"
            }
        });
        let rendered = json!({
            "Children": [
                {
                    "Children": [],
                    "RawValue": "42000",
                    "SensorId": "sensor-1",
                    "Text": "Test Temp",
                    "Type": "Temperature",
                    "Value": "42.00 °C"
                }
            ]
        });
        let state = make_live_state(rendered, raw);

        let app = test::init_service(
            App::new()
                .app_data(actix_web::web::Data::new(state))
                .configure(init_routes),
        )
        .await;

        let compare_response =
            test::call_service(&app, test::TestRequest::get().uri("/compare").to_request()).await;
        assert!(compare_response.status().is_success());
        let compare_body = test::read_body(compare_response).await;
        let compare_text = String::from_utf8(compare_body.to_vec()).expect("utf8");
        assert!(compare_text.contains("\"raw_top_keys_count\""));
        assert!(compare_text.contains("\"only_in_raw\""));
        assert!(compare_text.contains("\"only_in_ohm\":[]"));
        assert!(!compare_text.contains("\"Children\""));
        assert!(compare_text.contains("\"sensor-1\""));

        let view_response = test::call_service(
            &app,
            test::TestRequest::get().uri("/compare/view").to_request(),
        )
        .await;
        assert!(view_response.status().is_success());
        let view_body = test::read_body(view_response).await;
        let view_text = String::from_utf8(view_body.to_vec()).expect("utf8");
        assert!(view_text.contains("data.json"));
        assert!(view_text.contains("DATA"));
        assert!(view_text.contains("Children"));
        assert!(view_text.contains("42.00"));
        assert!(view_text.contains("sensor-1"));
    }

    #[actix_web::test]
    async fn compare_schema_ignores_live_value_drift_when_paths_and_units_match() {
        let _cwd_lock = cwd_mutex().lock().await;
        let tempdir = tempdir().expect("tempdir");
        let orig_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(tempdir.path()).expect("set cwd");
        let _cwd_guard = CwdGuard(orig_cwd);

        let ohm = json!({
            "Children": [{
                "Text": "CPU",
                "Type": "Temperature",
                "SensorId": "/test/temperature/0",
                "Min": "36.0 °C",
                "Value": "46.0 °C",
                "Max": "49.0 °C",
                "RawMin": "36.0 °C",
                "RawValue": "46.0 °C",
                "RawMax": "49.0 °C",
                "Children": []
            }]
        });
        std::fs::write(
            tempdir.path().join("ohm.json"),
            serde_json::to_string(&ohm).expect("serialize ohm"),
        )
        .expect("write ohm.json");

        let rendered = json!({
            "Children": [{
                "Text": "CPU",
                "Type": "Temperature",
                "SensorId": "/test/temperature/0",
                "Min": "40.0 °C",
                "Value": "55.5 °C",
                "Max": "61.0 °C",
                "RawMin": "40.0 °C",
                "RawValue": "55.5 °C",
                "RawMax": "61.0 °C",
                "Children": []
            }]
        });
        let state = make_live_state(rendered, json!({}));

        let app = test::init_service(
            App::new()
                .app_data(actix_web::web::Data::new(state))
                .configure(init_routes),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::get().uri("/compare/schema").to_request(),
        )
        .await;
        assert!(response.status().is_success());

        let body = test::read_body(response).await;
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json response");
        assert_eq!(
            value.get("compatible").and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            value
                .get("unit_mismatches")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(0)
        );
        assert_eq!(
            value
                .get("metadata_mismatches")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(0)
        );
    }

    #[actix_web::test]
    async fn compare_schema_flags_unit_mismatches() {
        let _cwd_lock = cwd_mutex().lock().await;
        let tempdir = tempdir().expect("tempdir");
        let orig_cwd = std::env::current_dir().expect("cwd");
        std::env::set_current_dir(tempdir.path()).expect("set cwd");
        let _cwd_guard = CwdGuard(orig_cwd);

        let ohm = json!({
            "Children": [{
                "Text": "CPU",
                "Type": "Temperature",
                "SensorId": "/test/temperature/0",
                "Value": "46.0 °C",
                "Children": []
            }]
        });
        std::fs::write(
            tempdir.path().join("ohm.json"),
            serde_json::to_string(&ohm).expect("serialize ohm"),
        )
        .expect("write ohm.json");

        let rendered = json!({
            "Children": [{
                "Text": "CPU",
                "Type": "Temperature",
                "SensorId": "/test/temperature/0",
                "Value": "46.0 V",
                "Children": []
            }]
        });
        let state = make_live_state(rendered, json!({}));

        let app = test::init_service(
            App::new()
                .app_data(actix_web::web::Data::new(state))
                .configure(init_routes),
        )
        .await;

        let response = test::call_service(
            &app,
            test::TestRequest::get().uri("/compare/schema").to_request(),
        )
        .await;
        assert!(response.status().is_success());

        let body = test::read_body(response).await;
        let value: serde_json::Value = serde_json::from_slice(&body).expect("json response");
        assert_eq!(
            value.get("compatible").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            value
                .get("unit_mismatches")
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(1)
        );
    }

    #[actix_web::test]
    async fn schema_unit_comparison_treats_unavailable_and_scaled_throughput_as_compatible() {
        assert!(units_are_compatible("%", "-"));
        assert!(units_are_compatible("MB/s", "KB/s"));
        assert!(units_are_compatible("KB/s", "MB/s"));
        assert!(!units_are_compatible("V", "°C"));
    }
}
