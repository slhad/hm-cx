// cleaned unused imports
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
// std::path not used
use actix_web::rt;
use actix_web::rt::task::spawn_blocking;
use tokio::sync::Notify;
use std::process::Command;
use std::sync::{Arc, RwLock};
use std::time::Duration;

const OHM_ASSET: &str = include_str!("../assets/openhardwaremonitor_localhost_8085_data.json");

#[derive(Debug, Serialize, Deserialize)]
pub struct MappingConfig {
    pub mappings: Vec<MappingEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MappingEntry {
    pub ohm: String,
    pub text: String,
    #[serde(rename = "type")]
    pub sensor_type: String,
    pub source: SensorSource,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SensorSource {
    pub kind: String, // "sysfs" or "sensors_json"
    // kind == sysfs => path set
    pub path: Option<String>,
    // kind == sensors_json => chip and key set
    pub chip: Option<String>,
    pub key: Option<String>,
}

// Public entry: generate a config.yml mapping OHM sensor ids to system sensors
pub fn generate_config() -> Result<(), String> {
    // Parse OHM asset and collect sensors
    let v: Value = serde_json::from_str(OHM_ASSET).map_err(|e| format!("asset parse: {}", e))?;
    let mut ohm_sensors = Vec::new();
    collect_ohm_sensors(&v, &mut ohm_sensors);

    // Collect sysfs sensors
    let sysfs = collect_sysfs_sensors();

    // Collect sensors -j output (if available)
    let sensors_json = collect_sensors_cmd();

    let mut mappings = Vec::new();

    for s in ohm_sensors {
        // Try match by index when SensorId ends with /type/N
        let mut chosen: Option<SensorSource> = None;
        if let Some((t, idx)) = parse_type_index(&s.ohm) {
            // prefer sysfs matching filename containing type+index
            let target = format!("{}{}", t_prefix(&t), idx);
            if let Some(path) = sysfs.iter().find_map(|(p, _meta)| {
                if p.contains(&target) {
                    Some(p.clone())
                } else {
                    None
                }
            }) {
                chosen = Some(SensorSource {
                    kind: "sysfs".into(),
                    path: Some(path),
                    chip: None,
                    key: None,
                });
            }
        }

        // Fallback: match by text in sysfs labels or filenames
        if chosen.is_none() {
            let text_norm = normalize_compact(&s.text);
            let text_tokens = normalize_tokens(&s.text);
            if let Some((p, _)) = sysfs.iter().find(|(_, meta)| {
                meta.iter().any(|(k, v)| {
                    let v_compact = normalize_compact(v);
                    let k_compact = normalize_compact(k);
                    // compact contains match or any token matches
                    v_compact.contains(&text_norm)
                        || k_compact.contains(&text_norm)
                        || text_tokens
                            .iter()
                            .any(|t| v_compact.contains(t) || k_compact.contains(t))
                })
            }) {
                chosen = Some(SensorSource {
                    kind: "sysfs".into(),
                    path: Some(p.clone()),
                    chip: None,
                    key: None,
                });
            }
        }

        // Fallback to sensors -j json
        if chosen.is_none() {
            if let Some((chip, key)) = find_in_sensors_json(&sensors_json, &s.text) {
                chosen = Some(SensorSource {
                    kind: "sensors_json".into(),
                    path: None,
                    chip: Some(chip),
                    key: Some(key),
                });
            }
        }

        if let Some(source) = chosen {
            mappings.push(MappingEntry {
                ohm: s.ohm,
                text: s.text,
                sensor_type: s.sensor_type,
                source,
            });
        }
    }

    let cfg = MappingConfig { mappings };
    let yaml = serde_yaml::to_string(&cfg).map_err(|e| format!("yaml serialize: {}", e))?;

    let mut f = fs::File::create("config.yml").map_err(|e| format!("create config.yml: {}", e))?;
    f.write_all(yaml.as_bytes())
        .map_err(|e| format!("write config.yml: {}", e))?;
    println!("Wrote config.yml with {} mappings", cfg.mappings.len());
    Ok(())
}

pub fn load_config_from_file(path: &str) -> Result<MappingConfig, String> {
    let s = fs::read_to_string(path).map_err(|e| format!("read {}: {}", path, e))?;
    serde_yaml::from_str(&s).map_err(|e| format!("yaml parse {}: {}", path, e))
}

/// Start a background sampler that updates an in-memory snapshot according to config.yml.
/// Returns a tuple: (Arc<RwLock<serde_json::Value>> snapshot, Arc<Notify> shutdown_notify).
pub fn init_live_state(
    config_path: &str,
    poll_interval_ms: u64,
) -> (Arc<RwLock<serde_json::Value>>, Arc<Notify>) {
    let snapshot = Arc::new(RwLock::new(serde_json::Value::Null));
    let snap_clone = snapshot.clone();
    let config_path = config_path.to_string();

    let shutdown_notify = Arc::new(Notify::new());
    let notify_clone = shutdown_notify.clone();

    // Perform an initial sampling synchronously so the snapshot is populated immediately.
    {
        let mut result = serde_json::Map::new();
        if let Ok(cfg) = load_config_from_file(&config_path) {
            let sensors_json = collect_sensors_cmd();
            for m in cfg.mappings.iter() {
                let key = &m.ohm;
                let mut value_json = serde_json::Value::Null;

                if m.source.kind == "sysfs" {
                    if let Some(p) = &m.source.path {
                        if let Ok(s) = fs::read_to_string(p) {
                            if let Ok(mut v) = s.trim().parse::<f64>() {
                                if v.abs() > 10000.0 {
                                    v /= 1000.0;
                                }
                                if m
                                    .sensor_type
                                    .to_lowercase()
                                    .contains("temperature")
                                    && v.abs() > 1000.0
                                {
                                    v /= 1000.0;
                                }
                                value_json = json!({"value": v, "text": m.text, "type": m.sensor_type});
                            } else {
                                value_json = json!({"raw": s.trim(), "text": m.text, "type": m.sensor_type});
                            }
                        }
                    }
                } else if m.source.kind == "sensors_json" {
                    if let (Some(chip), Some(key)) = (&m.source.chip, &m.source.key) {
                        if let Some(v) = extract_from_sensors_json(&sensors_json, chip, key) {
                            value_json = json!({"value": v, "text": m.text, "type": m.sensor_type});
                        }
                    }
                }

                if !value_json.is_null() {
                    result.insert(key.clone(), value_json);
                }
            }
        }

        let mut w = snapshot.write().unwrap();
        *w = serde_json::Value::Object(result);
    }

    // Spawn an async background task on the Actix runtime. Any blocking work (fs or external commands)
    // runs inside `rt::spawn_blocking` so the async reactor is not blocked.
    rt::spawn(async move {
        loop {
            let cfg_path = config_path.clone();

            // run blocking sampling in a threadpool
            let sampling_map = spawn_blocking(move || {
                let mut result = serde_json::Map::new();

                let cfg = match load_config_from_file(&cfg_path) {
                    Ok(c) => c,
                    Err(_) => return result,
                };

                let sensors_json = collect_sensors_cmd();

                for m in cfg.mappings.iter() {
                    let key = &m.ohm;
                    let mut value_json = serde_json::Value::Null;

                    if m.source.kind == "sysfs" {
                        if let Some(p) = &m.source.path {
                            if let Ok(s) = fs::read_to_string(p) {
                                if let Ok(mut v) = s.trim().parse::<f64>() {
                                    if v.abs() > 10000.0 {
                                        v /= 1000.0;
                                    }
                                    if m
                                        .sensor_type
                                        .to_lowercase()
                                        .contains("temperature")
                                        && v.abs() > 1000.0
                                    {
                                        v /= 1000.0;
                                    }
                                    value_json = json!({"value": v, "text": m.text, "type": m.sensor_type});
                                } else {
                                    value_json = json!({"raw": s.trim(), "text": m.text, "type": m.sensor_type});
                                }
                            }
                        }
                    } else if m.source.kind == "sensors_json" {
                        if let (Some(chip), Some(key)) = (&m.source.chip, &m.source.key) {
                            if let Some(v) = extract_from_sensors_json(&sensors_json, chip, key) {
                                value_json = json!({"value": v, "text": m.text, "type": m.sensor_type});
                            }
                        }
                    }

                    if !value_json.is_null() {
                        result.insert(key.clone(), value_json);
                    }
                }

                result
            })
            .await
            .unwrap_or_default();

            {
                let mut w = snap_clone.write().unwrap();
                *w = serde_json::Value::Object(sampling_map);
            }

            // Wait for either the poll interval or a shutdown notification
            tokio::select! {
                _ = rt::time::sleep(Duration::from_millis(poll_interval_ms)) => {},
                _ = notify_clone.notified() => {
                    break;
                }
            }
        }
    });

    (snapshot, shutdown_notify)
}

fn extract_from_sensors_json(sensors_json: &Value, chip: &str, key: &str) -> Option<f64> {
    if sensors_json.is_null() {
        return None;
    }
    if let Some(obj) = sensors_json.as_object() {
        if let Some(chip_obj) = obj.get(chip).and_then(|v| v.as_object()) {
            if let Some(kv) = chip_obj.get(key) {
                if let Some(s) = kv.as_str() {
                    if let Ok(mut v) = s
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .replace(',', ".")
                        .parse::<f64>()
                    {
                        if v.abs() > 10000.0 {
                            v /= 1000.0;
                        }
                        return Some(v);
                    }
                } else if kv.is_number() {
                    return kv.as_f64();
                }
            }
        }
    }
    None
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn normalize_compact(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

fn normalize_tokens(s: &str) -> Vec<String> {
    s.split_whitespace()
        .map(|tok| {
            tok.to_lowercase()
                .chars()
                .filter(|c| c.is_alphanumeric())
                .collect()
        })
        .collect()
}

fn t_prefix(t: &str) -> &'static str {
    match t.to_lowercase().as_str() {
        "temperature" => "temp",
        "fan" => "fan",
        "voltage" | "power" => "in",
        _ => "",
    }
}

fn parse_type_index(ohm: &str) -> Option<(String, String)> {
    // e.g. /lpc/it8688e/0/temperature/2 => ("temperature", "2")
    let parts: Vec<&str> = ohm.split('/').filter(|p| !p.is_empty()).collect();
    if parts.len() >= 2 {
        let len = parts.len();
        let t = parts[len - 2].to_string();
        let idx = parts[len - 1].to_string();
        Some((t, idx))
    } else {
        None
    }
}

fn collect_ohm_sensors(v: &Value, out: &mut Vec<OhmSensor>) {
    if let Some(obj) = v.as_object() {
        if let Some(sensor_id) = obj.get("SensorId").and_then(|s| s.as_str()) {
            let text = obj
                .get("Text")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let sensor_type = obj
                .get("Type")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            out.push(OhmSensor {
                ohm: sensor_id.to_string(),
                text,
                sensor_type,
            });
        }
        if let Some(children) = obj.get("Children") {
            if let Some(arr) = children.as_array() {
                for c in arr {
                    collect_ohm_sensors(c, out);
                }
            }
        }
    } else if let Some(arr) = v.as_array() {
        for c in arr {
            collect_ohm_sensors(c, out);
        }
    }
}

#[derive(Debug)]
struct OhmSensor {
    ohm: String,
    text: String,
    sensor_type: String,
}

fn collect_sysfs_sensors() -> HashMap<String, Vec<(String, String)>> {
    let mut map = HashMap::new();
    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                let mut meta = Vec::new();
                let name_path = path.join("name");
                if let Ok(name) = fs::read_to_string(&name_path) {
                    meta.push(("name".to_string(), name.trim().to_string()));
                }
                if let Ok(files) = fs::read_dir(&path) {
                    for f in files.flatten() {
                        if let Some(fname) = f.file_name().to_str() {
                            if fname.ends_with("_input") || fname.ends_with("_label") {
                                if let Ok(content) = fs::read_to_string(f.path()) {
                                    meta.push((fname.to_string(), content.trim().to_string()));
                                }
                            }
                        }
                    }
                }
                // store path strings to each *_input as keys too
                if let Ok(files) = fs::read_dir(&path) {
                    for f in files.flatten() {
                        if let Some(fname) = f.file_name().to_str() {
                            if fname.ends_with("_input") {
                                let p = f.path().to_string_lossy().to_string();
                                map.insert(p, meta.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

fn collect_sensors_cmd() -> Value {
    // Calling external `sensors -j` may fail in test or environments without lm-sensors.
    // Keep function simple: try running it, but return Null on error.
    match Command::new("sensors").arg("-j").output() {
        Ok(output) => {
            if output.status.success() {
                if let Ok(s) = String::from_utf8(output.stdout) {
                    if let Ok(v) = serde_json::from_str(&s) {
                        return v;
                    }
                }
            }
            Value::Null
        }
        Err(_) => Value::Null,
    }
}

fn find_in_sensors_json(sensors_json: &Value, text: &str) -> Option<(String, String)> {
    if sensors_json.is_null() {
        return None;
    }
    let tnorm = normalize(text);
    if let Some(obj) = sensors_json.as_object() {
        for (chip, values) in obj {
            if let Some(vals) = values.as_object() {
                for (k, v) in vals {
                    let label = k.to_lowercase();
                    let vstr = v.as_str().unwrap_or("").to_lowercase();
                    if label.contains(&tnorm) || vstr.contains(&tnorm) {
                        return Some((chip.clone(), k.clone()));
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    // helper to construct sensors-json like Value for other tests if needed
    // mark as allowed dead code to avoid unused function warning when not referenced
    #[allow(dead_code)]
    fn make_sensors_json() -> Value {
        json!({
            "it8688-isa-0a20": {
                "temp2_input": "42.0 C",
                "fan1_input": "1200 RPM"
            },
            "coretemp-isa-0000": {
                "Package id 0": "55.0 C"
            }
        })
    }

    #[test]
    fn test_collect_ohm_sensors_simple() {
        let v = json!({
            "SensorId": "/lpc/it8688e/0/temperature/2",
            "Text": "CPU Temp",
            "Type": "Temperature",
        });

        let mut out = Vec::new();
        collect_ohm_sensors(&v, &mut out);
        assert_eq!(out.len(), 1);
        let s = &out[0];
        assert_eq!(s.ohm, "/lpc/it8688e/0/temperature/2");
        assert_eq!(s.text, "CPU Temp");
        assert_eq!(s.sensor_type, "Temperature");
    }

    #[test]
    fn test_collect_ohm_sensors_nested_children() {
        let v = json!({
            "Children": [
                {"SensorId": "/foo/1/voltage/0", "Text": "Vcore", "Type": "Voltage"},
                {"SensorId": "/bar/1/fan/1", "Text": "Chassis Fan", "Type": "Fan"}
            ]
        });

        let mut out = Vec::new();
        collect_ohm_sensors(&v, &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].ohm, "/foo/1/voltage/0");
        assert_eq!(out[0].text, "Vcore");
        assert_eq!(out[0].sensor_type, "Voltage");
        assert_eq!(out[1].ohm, "/bar/1/fan/1");
        assert_eq!(out[1].text, "Chassis Fan");
        assert_eq!(out[1].sensor_type, "Fan");
    }

    #[test]
    fn test_parse_type_index_examples() {
        let s = "/lpc/it8688e/0/temperature/2";
        let r = parse_type_index(s).expect("should parse");
        assert_eq!(r.0, "temperature");
        assert_eq!(r.1, "2");

        let s2 = "//foo/bar/voltage/0";
        let r2 = parse_type_index(s2).expect("should parse");
        assert_eq!(r2.0, "voltage");
        assert_eq!(r2.1, "0");
    }

    #[test]
    fn test_find_in_sensors_json_with_mocked_json() {
        // Create a small sensors -j like object to test matching without calling `sensors` binary
        let sensors_json = json!({
            "it8688-isa-0a20": {
                "temp2_input": "42.0 C",
                "fan1_input": "1200 RPM"
            },
            "coretemp-isa-0000": {
                "Package id 0": "55.0 C"
            }
        });

        // match by key label
        let found = find_in_sensors_json(&sensors_json, "temp2");
        assert!(found.is_some());
        let (chip, key) = found.unwrap();
        assert_eq!(chip, "it8688-isa-0a20");
        assert_eq!(key, "temp2_input");

        // match by value string when normalized (value contains numeric/units, so matching label instead)
        let found2 = find_in_sensors_json(&sensors_json, "Package");
        assert!(found2.is_some());
        let (chip2, key2) = found2.unwrap();
        assert_eq!(chip2, "coretemp-isa-0000");
        assert_eq!(key2, "Package id 0");
    }
}
