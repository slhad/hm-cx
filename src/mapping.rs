// cleaned unused imports
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
// std::path not used
// actix runtime imports were used in earlier designs; keep mapping independent of Actix.
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
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
) -> (Arc<RwLock<serde_json::Value>>, Arc<AtomicBool>) {
    let snapshot = Arc::new(RwLock::new(serde_json::Value::Null));
    let snap_clone = snapshot.clone();
    let config_path = config_path.to_string();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();

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
                            let raw = s.trim().to_string();
                            if let Ok(mut v) = raw.parse::<f64>() {
                                // Normalize common scales: millidegrees/millivolts -> divide by 1000
                                if v.abs() > 1000.0 {
                                    v /= 1000.0;
                                }
                                // Format according to sensor type to match asset style
                                let formatted = format_sensor_value(v, &m.sensor_type);
                                value_json = json!({
                                    "Value": formatted,
                                    "RawValue": raw,
                                    "Text": m.text,
                                    "Type": m.sensor_type
                                });
                            } else {
                                // couldn't parse numeric: keep raw string
                                value_json = json!({"Value": raw.clone(), "RawValue": raw, "Text": m.text, "Type": m.sensor_type});
                            }
                        }
                    }
                } else if m.source.kind == "sensors_json" {
                    if let (Some(chip), Some(key)) = (&m.source.chip, &m.source.key) {
                        if let Some(v_raw) = extract_from_sensors_json(&sensors_json, chip, key) {
                            let mut v = v_raw;
                            if v.abs() > 1000.0 {
                                v /= 1000.0;
                            }
                            let formatted = format_sensor_value(v, &m.sensor_type);
                            value_json = json!({"Value": formatted, "RawValue": v_raw.to_string(), "Text": m.text, "Type": m.sensor_type});
                        }
                    }
                }

                if !value_json.is_null() {
                    result.insert(key.clone(), value_json);
                } else {
                    // Log when a mapping produced no value to help diagnose misconfigurations
                    tracing::warn!(ohm = %key, source = ?m.source, "mapping produced no value");
                }
            }
        }

        // Build an OHM asset-shaped Value and apply sampled values into it.
        let mut asset_val: Value = serde_json::from_str(OHM_ASSET).unwrap_or(Value::Null);
        apply_values_to_asset(&result, &mut asset_val);

        let mut w = snapshot.write().unwrap();
        *w = asset_val;
    }

    // Spawn a dedicated thread for sampling so init_live_state can be called before
    // the Actix runtime starts. The thread performs blocking IO and updates the snapshot.
    std::thread::spawn(move || {
        loop {
            let cfg_path = config_path.clone();

            let mut result = serde_json::Map::new();

            if let Ok(cfg) = load_config_from_file(&cfg_path) {
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
                                    if m.sensor_type.to_lowercase().contains("temperature")
                                        && v.abs() > 1000.0
                                    {
                                        v /= 1000.0;
                                    }
                                    value_json =
                                        json!({"value": v, "text": m.text, "type": m.sensor_type});
                                } else {
                                    value_json = json!({"raw": s.trim(), "text": m.text, "type": m.sensor_type});
                                }
                            }
                        }
                    } else if m.source.kind == "sensors_json" {
                        if let (Some(chip), Some(key)) = (&m.source.chip, &m.source.key) {
                            if let Some(v) = extract_from_sensors_json(&sensors_json, chip, key) {
                                value_json =
                                    json!({"value": v, "text": m.text, "type": m.sensor_type});
                            }
                        }
                    }

                    if !value_json.is_null() {
                        result.insert(key.clone(), value_json);
                    } else {
                        // Log when a mapping produced no value to help diagnose misconfigurations
                        tracing::warn!(ohm = %key, source = ?m.source, "mapping produced no value");
                    }
                }
            }

            {
                // Build asset-shaped Value and apply sampled values
                let mut asset_val: Value = serde_json::from_str(OHM_ASSET).unwrap_or(Value::Null);
                apply_values_to_asset(&result, &mut asset_val);
                let mut w = snap_clone.write().unwrap();
                *w = asset_val;
            }

            // sleep in small increments so we can break promptly when stop flag is set
            let mut waited = 0u64;
            while waited < poll_interval_ms {
                if stop_clone.load(Ordering::SeqCst) {
                    return;
                }
                let step = std::cmp::min(100, poll_interval_ms - waited);
                std::thread::sleep(Duration::from_millis(step));
                waited += step;
            }
        }
    });

    (snapshot, stop_flag)
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

pub fn format_sensor_value(v: f64, sensor_type: &str) -> String {
    // Match the human-readable formatting used in the bundled asset.
    match sensor_type.to_lowercase().as_str() {
        t if t.contains("temperature") => format!("{:.2} °C", v),
        t if t.contains("voltage") => format!("{:.3} V", v),
        t if t.contains("power") => format!("{:.1} W", v),
        t if t.contains("fan") => format!("{:.0} RPM", v),
        t if t.contains("load") => format!("{:.1}", v),
        _ => format!("{:.2}", v),
    }
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

// Apply sampled values (a map from SensorId -> object) into the OHM asset structure.
// This walks the asset tree and, when a node with a `SensorId` is found, merges the
// sampled fields into that node so the resulting asset mirrors the bundled format.
fn apply_values_to_asset(sampled: &serde_json::Map<String, Value>, asset: &mut Value) {
    match asset {
        Value::Object(map) => {
            // If this object has a SensorId, attempt to apply sampled value
            if let Some(Value::String(sensor_id)) = map.get("SensorId") {
                if let Some(sample) = sampled.get(sensor_id) {
                    if let Some(sample_obj) = sample.as_object() {
                        for (k, v) in sample_obj.iter() {
                            map.insert(k.clone(), v.clone());
                        }
                    }
                }
            }

            // Recurse into children if present
            if let Some(children) = map.get_mut("Children") {
                if let Some(arr) = children.as_array_mut() {
                    for child in arr.iter_mut() {
                        apply_values_to_asset(sampled, child);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                apply_values_to_asset(sampled, item);
            }
        }
        _ => {}
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
