// cleaned unused imports
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
// std::path not used
// actix runtime imports were used in earlier designs; keep mapping independent of Actix.
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

const OHM_ASSET: &str = include_str!("../assets/openhardwaremonitor_localhost_8085_data.json");

#[derive(Debug)]
pub struct LiveState {
    pub rendered: Arc<RwLock<Value>>,
    pub raw: Arc<RwLock<Value>>,
}

pub fn load_bundled_raw_data() -> Value {
    let asset: Value = serde_json::from_str(OHM_ASSET).unwrap_or(Value::Null);
    let mut raw = serde_json::Map::new();
    collect_raw_sensor_entries(&asset, &mut raw);
    Value::Object(raw)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingConfig {
    pub mappings: Vec<MappingEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MappingEntry {
    pub ohm: String,
    pub text: String,
    #[serde(rename = "type")]
    pub sensor_type: String,
    pub source: SensorSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorSource {
    pub kind: String, // "sysfs", "sensors_json", "powercap_rapl", or "proc_cpuinfo"
    // kind == sysfs/powercap_rapl/proc_cpuinfo => path set
    pub path: Option<String>,
    // kind == sensors_json => chip and key set; kind == proc_cpuinfo => selector stored in key
    pub chip: Option<String>,
    pub key: Option<String>,
}

#[derive(Debug, Clone)]
struct ProcCpuInfoSample {
    core_mhz: HashMap<usize, f64>,
    average_mhz: f64,
}

#[derive(Debug, Clone)]
struct PowercapReading {
    energy_uj: u64,
    timestamp: Instant,
}

#[derive(Debug, Default)]
struct SampleCache {
    powercap_readings: HashMap<String, PowercapReading>,
}

fn sensor_type_key(sensor_type: &str) -> String {
    sensor_type.trim().to_ascii_lowercase()
}

fn sysfs_value_family(path: &str) -> Option<(String, usize)> {
    let file_name = std::path::Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())?;

    let mut family_end = 0usize;
    for ch in file_name.chars() {
        if ch.is_ascii_alphabetic() {
            family_end += ch.len_utf8();
        } else {
            break;
        }
    }

    if family_end == 0 || family_end >= file_name.len() {
        return None;
    }

    let digits_end = file_name[family_end..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .map(char::len_utf8)
        .sum::<usize>();
    if digits_end == 0 {
        return None;
    }

    let family = file_name[..family_end].to_string();
    let channel = file_name[family_end..family_end + digits_end]
        .parse::<usize>()
        .ok()?;
    Some((family, channel))
}

fn supported_sysfs_families(sensor_type: &str) -> &'static [&'static str] {
    match sensor_type_key(sensor_type).as_str() {
        "temperature" => &["temp"],
        "voltage" => &["in"],
        "current" => &["curr"],
        "fan" => &["fan"],
        "clock" => &["freq"],
        "power" => &["power"],
        "control" | "level" => &["pwm"],
        _ => &[],
    }
}

fn sysfs_path_matches_sensor_type(path: &str, sensor_type: &str) -> bool {
    let Some((family, _)) = sysfs_value_family(path) else {
        return false;
    };

    supported_sysfs_families(sensor_type)
        .iter()
        .any(|candidate| family == *candidate)
}

fn expected_sysfs_channel(ohm: &str, sensor_type: &str) -> Option<usize> {
    let (_, index) = parse_type_index(ohm)?;
    let index = index.parse::<usize>().ok()?;

    match sensor_type_key(sensor_type).as_str() {
        // hwmon numbers these channels from 1.
        "temperature" | "current" | "fan" | "clock" | "control" | "level" => Some(index + 1),
        // hwmon voltage channels already use zero-based numbering like in0_input.
        "voltage" => Some(index),
        _ => None,
    }
}

fn lpc_expected_device_rank(sensor_id: &str) -> Option<usize> {
    if sensor_id.starts_with("/lpc/it8688e/") {
        return Some(1);
    }
    if sensor_id.starts_with("/lpc/it8792e/") {
        return Some(2);
    }
    None
}

fn sensor_device_hint_score(sensor_id: &str, meta: &[(String, String)]) -> usize {
    let name = meta
        .iter()
        .find(|(key, _)| key == "name")
        .map(|(_, value)| normalize(value))
        .unwrap_or_default();

    if sensor_id.starts_with("/gpu-amd/") && name.contains("amdgpu") {
        return 40;
    }
    if sensor_id.starts_with("/usbhid/") && name.contains("octo") {
        return 35;
    }
    if sensor_id.starts_with("/lpc/") && name.contains("it") {
        let device_rank = meta
            .iter()
            .find(|(key, _)| key == "device_rank")
            .and_then(|(_, value)| value.parse::<usize>().ok());
        let rank_bonus = match (lpc_expected_device_rank(sensor_id), device_rank) {
            (Some(expected), Some(actual)) if expected == actual => 25,
            _ => 0,
        };
        return 25 + rank_bonus;
    }
    if sensor_id.starts_with("/amdcpu/") && name.contains("k10temp") {
        return 30;
    }
    if sensor_id.starts_with("/memory") && name.contains("jc42") {
        return 30;
    }
    if sensor_id.starts_with("/nvme/") && name.contains("nvme") {
        return 30;
    }

    0
}

fn first_number_in_text(text: &str) -> Option<usize> {
    let digits: String = text.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        digits.parse::<usize>().ok()
    }
}

fn label_aliases(sensor_id: &str, text: &str, sensor_type: &str) -> &'static [&'static str] {
    let text_norm = normalize(text);

    if sensor_id.starts_with("/gpu-amd/")
        && sensor_type_key(sensor_type) == "power"
        && text_norm.contains("package")
    {
        // AMD GPUs expose package power as PPT in hwmon.
        return &["ppt"];
    }

    &[]
}

fn semantic_tokens(text: &str) -> Vec<String> {
    normalize_tokens(text)
        .into_iter()
        .filter(|token| token.chars().any(|ch| ch.is_ascii_alphabetic()))
        .collect()
}

fn related_label_score(
    path: &str,
    meta: &[(String, String)],
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
) -> usize {
    let text_norm = normalize_compact(text);
    let text_tokens = semantic_tokens(text);
    let aliases = label_aliases(sensor_id, text, sensor_type);
    let Some((family, channel)) = sysfs_value_family(path) else {
        return 0;
    };

    let matching_label_key = format!("{}{}_label", family, channel);
    let number_hint = first_number_in_text(text);

    meta.iter().fold(0usize, |best, (key, value)| {
        if key == "name" {
            // Device hints are handled separately so generic names like `amdgpu` do not count as a label match.
            return best;
        }

        let key_norm = normalize_compact(key);
        let value_norm = normalize_compact(value);
        let same_channel = key == &matching_label_key;
        let channel_hit = number_hint.is_some_and(|number| number == channel);
        let contains_text = !text_norm.is_empty()
            && (value_norm.contains(&text_norm) || key_norm.contains(&text_norm));
        let token_hits = text_tokens
            .iter()
            .filter(|token| !token.is_empty())
            .filter(|token| value_norm.contains(*token) || key_norm.contains(*token))
            .count();
        let alias_hits = aliases
            .iter()
            .filter(|alias| value_norm.contains(**alias) || key_norm.contains(**alias))
            .count();
        let has_semantic_match = contains_text || token_hits > 0 || alias_hits > 0;

        let mut score = 0usize;
        if contains_text {
            score += if same_channel { 60 } else { 25 };
        }
        if token_hits > 0 {
            score += token_hits * if same_channel { 12 } else { 5 };
        }
        if alias_hits > 0 {
            score += alias_hits * if same_channel { 35 } else { 14 };
        }
        if same_channel && channel_hit && has_semantic_match {
            score += 20;
        }
        if sensor_type_key(sensor_type) == "fan" && same_channel && channel_hit {
            score += 10;
        }

        best.max(score)
    })
}

fn score_clock_candidate(
    path: &str,
    meta: &[(String, String)],
    sensor_id: &str,
    text: &str,
) -> Option<usize> {
    if sensor_id.starts_with("/amdcpu/") {
        // lm-sensors/hwmon on this host does not expose per-core CPU clocks.
        return None;
    }

    if !sensor_id.starts_with("/gpu-amd/") {
        return None;
    }

    let (family, channel) = sysfs_value_family(path)?;
    if family != "freq" {
        return None;
    }

    let device_score = sensor_device_hint_score(sensor_id, meta);
    if device_score == 0 {
        return None;
    }

    let text_norm = normalize(text);
    if text_norm.contains("gpucore") && channel == 1 {
        return Some(200 + device_score);
    }
    if text_norm.contains("gpumemory") && channel == 2 {
        return Some(200 + device_score);
    }

    None
}

fn score_sysfs_candidate(
    path: &str,
    meta: &[(String, String)],
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
) -> Option<usize> {
    if !sysfs_path_matches_sensor_type(path, sensor_type) {
        return None;
    }

    if sensor_type_key(sensor_type) == "clock" {
        return score_clock_candidate(path, meta, sensor_id, text);
    }

    let device_score = sensor_device_hint_score(sensor_id, meta);
    let label_score = related_label_score(path, meta, sensor_id, text, sensor_type);
    let mut score = device_score + label_score;

    if let (Some(expected_channel), Some((_, actual_channel))) = (
        expected_sysfs_channel(sensor_id, sensor_type),
        sysfs_value_family(path),
    ) {
        if expected_channel == actual_channel {
            score += 50;
        }
    }

    if matches!(
        sensor_type_key(sensor_type).as_str(),
        "clock" | "power" | "load" | "throughput" | "data" | "smalldata" | "timing" | "factor"
    ) && label_score == 0
    {
        return None;
    }

    Some(score)
}

fn select_sysfs_source(
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
    sysfs: &HashMap<String, Vec<(String, String)>>,
) -> Option<SensorSource> {
    let mut best: Option<(usize, &String)> = None;

    for (path, meta) in sysfs.iter() {
        let Some(score) = score_sysfs_candidate(path, meta, sensor_id, text, sensor_type) else {
            continue;
        };

        match &best {
            Some((best_score, best_path))
                if *best_score > score
                    || (*best_score == score && best_path.as_str() <= path.as_str()) => {}
            _ => best = Some((score, path)),
        }
    }

    best.map(|(_, path)| SensorSource {
        kind: "sysfs".into(),
        path: Some(path.clone()),
        chip: None,
        key: None,
    })
}

fn collect_powercap_sensors() -> HashMap<String, Vec<(String, String)>> {
    let mut map = HashMap::new();
    let Ok(entries) = fs::read_dir("/sys/class/powercap") else {
        return map;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !file_name.starts_with("intel-rapl:") {
            continue;
        }

        let energy_path = path.join("energy_uj");
        if !energy_path.exists() {
            continue;
        }

        let mut meta = Vec::new();
        if let Ok(name) = fs::read_to_string(path.join("name")) {
            meta.push(("name".to_string(), name.trim().to_string()));
        }
        map.insert(energy_path.to_string_lossy().to_string(), meta);
    }

    map
}

fn select_proc_cpuinfo_source(
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
) -> Option<SensorSource> {
    if sensor_type_key(sensor_type) != "clock" || !sensor_id.starts_with("/amdcpu/") {
        return None;
    }

    let (_, index) = parse_type_index(sensor_id)?;
    let index = index.parse::<usize>().ok()?;
    let key = match index {
        0 if normalize(text).contains("busspeed") => "bus_speed".to_string(),
        1 => "core_average".to_string(),
        2 => "core_average".to_string(),
        idx if idx >= 3 => {
            let core_index = (idx - 3) / 2;
            format!("core:{}", core_index)
        }
        _ => return None,
    };

    Some(SensorSource {
        kind: "proc_cpuinfo".into(),
        path: Some("/proc/cpuinfo".to_string()),
        chip: None,
        key: Some(key),
    })
}

fn select_powercap_source(
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
    powercap: &HashMap<String, Vec<(String, String)>>,
) -> Option<SensorSource> {
    if sensor_type_key(sensor_type) != "power" || sensor_id != "/amdcpu/0/power/0" {
        return None;
    }

    let text_norm = normalize(text);
    if !text_norm.contains("package") {
        return None;
    }

    let mut best: Option<&String> = None;
    for (path, meta) in powercap {
        let domain_name = meta
            .iter()
            .find(|(key, _)| key == "name")
            .map(|(_, value)| normalize(value))
            .unwrap_or_default();

        if !domain_name.contains("package") {
            continue;
        }

        match best {
            Some(best_path) if best_path.as_str() <= path.as_str() => {}
            _ => best = Some(path),
        }
    }

    best.map(|path| SensorSource {
        kind: "powercap_rapl".into(),
        path: Some(path.clone()),
        chip: None,
        key: None,
    })
}

fn source_is_usable(source: &SensorSource, sensor_type: &str) -> bool {
    match source.kind.as_str() {
        "sysfs" => source.path.as_deref().is_some_and(|path| {
            std::path::Path::new(path).exists() && sysfs_path_matches_sensor_type(path, sensor_type)
        }),
        "powercap_rapl" => source
            .path
            .as_deref()
            .is_some_and(|path| std::path::Path::new(path).exists()),
        "proc_cpuinfo" => source
            .path
            .as_deref()
            .is_some_and(|path| std::path::Path::new(path).exists() && source.key.is_some()),
        "sensors_json" => source.chip.is_some() && source.key.is_some(),
        _ => false,
    }
}

fn resolve_mapping_source(
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
    powercap: &HashMap<String, Vec<(String, String)>>,
    sysfs: &HashMap<String, Vec<(String, String)>>,
    sensors_json: &Value,
) -> Option<SensorSource> {
    if let Some(source) = select_powercap_source(sensor_id, text, sensor_type, powercap) {
        return Some(source);
    }

    if let Some(source) = select_proc_cpuinfo_source(sensor_id, text, sensor_type) {
        return Some(source);
    }

    if let Some(source) = select_sysfs_source(sensor_id, text, sensor_type, sysfs) {
        return Some(source);
    }

    find_in_sensors_json(sensors_json, sensor_id, text, sensor_type).map(|(chip, key)| {
        SensorSource {
            kind: "sensors_json".into(),
            path: None,
            chip: Some(chip),
            key: Some(key),
        }
    })
}

fn reconcile_mapping_config(
    cfg: &mut MappingConfig,
    powercap: &HashMap<String, Vec<(String, String)>>,
    sysfs: &HashMap<String, Vec<(String, String)>>,
    sensors_json: &Value,
) {
    let mut reconciled = Vec::with_capacity(cfg.mappings.len());

    for mut mapping in std::mem::take(&mut cfg.mappings) {
        if source_is_usable(&mapping.source, &mapping.sensor_type) {
            reconciled.push(mapping);
            continue;
        }

        if let Some(source) = resolve_mapping_source(
            &mapping.ohm,
            &mapping.text,
            &mapping.sensor_type,
            powercap,
            sysfs,
            sensors_json,
        ) {
            mapping.source = source;
            reconciled.push(mapping);
            continue;
        }

        tracing::warn!(
            ohm = %mapping.ohm,
            sensor_type = %mapping.sensor_type,
            "dropping unresolved mapping with incompatible source"
        );
    }

    cfg.mappings = reconciled;
}

// Public entry: generate a config.yml mapping OHM sensor ids to system sensors
pub fn generate_config() -> Result<(), String> {
    // Parse OHM asset and collect sensors
    let v: Value = serde_json::from_str(OHM_ASSET).map_err(|e| format!("asset parse: {}", e))?;
    let mut ohm_sensors = Vec::new();
    collect_ohm_sensors(&v, &mut ohm_sensors);

    let powercap = collect_powercap_sensors();

    // Collect sysfs sensors
    let sysfs = collect_sysfs_sensors();

    // Collect sensors -j output (if available)
    let sensors_json = collect_sensors_cmd();

    let mut mappings = Vec::new();

    for s in ohm_sensors {
        if let Some(source) = resolve_mapping_source(
            &s.ohm,
            &s.text,
            &s.sensor_type,
            &powercap,
            &sysfs,
            &sensors_json,
        ) {
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

fn parse_proc_cpuinfo_sample(contents: &str) -> Option<ProcCpuInfoSample> {
    let mut core_mhz = HashMap::new();

    for block in contents.split("\n\n") {
        let mut processor: Option<usize> = None;
        let mut core_id: Option<usize> = None;
        let mut mhz: Option<f64> = None;

        for line in block.lines() {
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "processor" => processor = value.parse::<usize>().ok(),
                "core id" => core_id = value.parse::<usize>().ok(),
                "cpu MHz" => mhz = value.parse::<f64>().ok(),
                _ => {}
            }
        }

        let Some(mhz) = mhz else {
            continue;
        };
        let core_id = core_id.or(processor)?;
        core_mhz
            .entry(core_id)
            .and_modify(|current: &mut f64| {
                if mhz > *current {
                    *current = mhz;
                }
            })
            .or_insert(mhz);
    }

    if core_mhz.is_empty() {
        return None;
    }

    let average_mhz = core_mhz.values().sum::<f64>() / core_mhz.len() as f64;
    Some(ProcCpuInfoSample {
        core_mhz,
        average_mhz,
    })
}

fn read_powercap_max_energy_uj(path: &str) -> Option<u64> {
    let parent = std::path::Path::new(path).parent()?;
    let max_path = parent.join("max_energy_range_uj");
    fs::read_to_string(max_path)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
}

fn compute_powercap_watts(
    previous_energy_uj: u64,
    current_energy_uj: u64,
    elapsed: Duration,
    max_energy_range_uj: Option<u64>,
) -> Option<f64> {
    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs <= 0.0 {
        return None;
    }

    let delta_energy_uj = if current_energy_uj >= previous_energy_uj {
        current_energy_uj - previous_energy_uj
    } else {
        let max_energy_range_uj = max_energy_range_uj?;
        max_energy_range_uj.saturating_sub(previous_energy_uj) + current_energy_uj
    };

    Some(delta_energy_uj as f64 / elapsed_secs / 1_000_000.0)
}

fn sample_powercap_rapl_value(path: &str, cache: &mut SampleCache) -> Option<(f64, String)> {
    let now = Instant::now();
    let current_energy_uj = fs::read_to_string(path).ok()?.trim().parse::<u64>().ok()?;
    let previous = cache.powercap_readings.insert(
        path.to_string(),
        PowercapReading {
            energy_uj: current_energy_uj,
            timestamp: now,
        },
    )?;

    let watts = compute_powercap_watts(
        previous.energy_uj,
        current_energy_uj,
        now.duration_since(previous.timestamp),
        read_powercap_max_energy_uj(path),
    )?;
    let raw_power_uw = (watts * 1_000_000.0).round() as u64;
    Some((watts, raw_power_uw.to_string()))
}

fn sample_proc_cpuinfo() -> Option<ProcCpuInfoSample> {
    let contents = fs::read_to_string("/proc/cpuinfo").ok()?;
    parse_proc_cpuinfo_sample(&contents)
}

fn proc_cpuinfo_value(sample: &ProcCpuInfoSample, key: &str) -> Option<f64> {
    match key {
        "bus_speed" => Some(100.0),
        "core_average" => Some(sample.average_mhz),
        _ => key
            .strip_prefix("core:")
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| sample.core_mhz.get(&index).copied()),
    }
}

fn read_sysfs_values(paths: &[String]) -> HashMap<String, String> {
    if paths.is_empty() {
        return HashMap::new();
    }

    let worker_count = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(4)
        .min(paths.len());
    let chunk_size = paths.len().div_ceil(worker_count);
    let (tx, rx) = mpsc::channel();

    std::thread::scope(|scope| {
        for chunk in paths.chunks(chunk_size) {
            let tx = tx.clone();
            scope.spawn(move || {
                for path in chunk {
                    if let Ok(raw_contents) = fs::read_to_string(path) {
                        let _ = tx.send((path.clone(), raw_contents));
                    }
                }
            });
        }
        drop(tx);

        let mut values = HashMap::new();
        for (path, raw_contents) in rx {
            values.insert(path, raw_contents);
        }
        values
    })
}

fn sample_mapping_values(
    cfg: &mut MappingConfig,
    cache: &mut SampleCache,
) -> serde_json::Map<String, Value> {
    let needs_reconcile = cfg
        .mappings
        .iter()
        .any(|mapping| !source_is_usable(&mapping.source, &mapping.sensor_type));
    let uses_sensors_json = cfg
        .mappings
        .iter()
        .any(|mapping| mapping.source.kind == "sensors_json");
    let uses_proc_cpuinfo = cfg
        .mappings
        .iter()
        .any(|mapping| mapping.source.kind == "proc_cpuinfo");

    let powercap = if needs_reconcile {
        collect_powercap_sensors()
    } else {
        HashMap::new()
    };
    let sysfs = if needs_reconcile {
        collect_sysfs_sensors()
    } else {
        HashMap::new()
    };
    let sensors_json = if needs_reconcile || uses_sensors_json {
        collect_sensors_cmd()
    } else {
        Value::Null
    };
    let proc_cpuinfo = if uses_proc_cpuinfo {
        sample_proc_cpuinfo()
    } else {
        None
    };

    if needs_reconcile {
        reconcile_mapping_config(cfg, &powercap, &sysfs, &sensors_json);
    }

    let unique_sysfs_paths = cfg
        .mappings
        .iter()
        .filter(|mapping| mapping.source.kind == "sysfs")
        .filter_map(|mapping| mapping.source.path.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let sysfs_values = read_sysfs_values(&unique_sysfs_paths);

    let mut result = serde_json::Map::new();

    for mapping in &cfg.mappings {
        let key = &mapping.ohm;
        let display_text = compatible_sensor_text(key, &mapping.sensor_type, &mapping.text);
        let mut value_json = Value::Null;

        if mapping.source.kind == "sysfs" {
            if let Some(path) = &mapping.source.path {
                if let Some(raw_contents) = sysfs_values.get(path) {
                    let raw = raw_contents.trim().to_string();
                    if let Ok(parsed) = raw.parse::<f64>() {
                        let value = normalize_sysfs_value(parsed, &mapping.sensor_type, path);
                        value_json = build_value_json(
                            Some(value),
                            Some(raw.clone()),
                            &display_text,
                            &mapping.sensor_type,
                        );
                    } else {
                        value_json = build_value_json(
                            None,
                            Some(raw.clone()),
                            &display_text,
                            &mapping.sensor_type,
                        );
                    }
                }
            }
        } else if mapping.source.kind == "powercap_rapl" {
            if let Some(path) = &mapping.source.path {
                if let Some((value, raw_value)) = sample_powercap_rapl_value(path, cache) {
                    value_json = build_value_json(
                        Some(value),
                        Some(raw_value),
                        &display_text,
                        &mapping.sensor_type,
                    );
                }
            }
        } else if mapping.source.kind == "proc_cpuinfo" {
            if let (Some(sample), Some(key_path)) = (&proc_cpuinfo, &mapping.source.key) {
                if let Some(value) = proc_cpuinfo_value(sample, key_path) {
                    value_json = build_value_json(
                        Some(value),
                        Some(format!("{:.6}", value)),
                        &display_text,
                        &mapping.sensor_type,
                    );
                }
            }
        } else if mapping.source.kind == "sensors_json" {
            if let (Some(chip), Some(key_path)) = (&mapping.source.chip, &mapping.source.key) {
                if let Some(raw_value) = extract_from_sensors_json(&sensors_json, chip, key_path) {
                    let value =
                        normalize_sensors_json_value(raw_value, &mapping.sensor_type, key_path);
                    value_json = build_value_json(
                        Some(value),
                        Some(raw_value.to_string()),
                        &display_text,
                        &mapping.sensor_type,
                    );
                }
            }
        }

        if !value_json.is_null() {
            result.insert(key.clone(), value_json);
        } else {
            tracing::warn!(ohm = %key, source = ?mapping.source, "mapping produced no value");
        }
    }

    result
}

/// Start a background sampler that updates an in-memory snapshot according to config.yml.
/// Returns a tuple: (shared live state, shutdown flag).
pub fn init_live_state(
    config_path: &str,
    poll_interval_ms: u64,
) -> (Arc<LiveState>, Arc<AtomicBool>) {
    let rendered_snapshot = Arc::new(RwLock::new(load_ohm_asset()));
    let raw_snapshot = Arc::new(RwLock::new(serde_json::Value::Object(
        serde_json::Map::new(),
    )));
    let live_state = Arc::new(LiveState {
        rendered: rendered_snapshot.clone(),
        raw: raw_snapshot.clone(),
    });
    let rendered_clone = rendered_snapshot.clone();
    let raw_clone = raw_snapshot.clone();
    let config_path = config_path.to_string();
    let mut initialized_min_max = HashSet::new();

    let stop_flag = Arc::new(AtomicBool::new(false));
    let stop_clone = stop_flag.clone();
    let mut sample_cache = SampleCache::default();

    // Perform an initial sampling synchronously so the snapshot is populated immediately.
    {
        let result = load_config_from_file(&config_path)
            .map(|mut cfg| sample_mapping_values(&mut cfg, &mut sample_cache))
            .unwrap_or_default();

        {
            let mut raw_w = raw_snapshot.write().unwrap();
            *raw_w = serde_json::Value::Object(result.clone());
        }

        let mut w = rendered_snapshot.write().unwrap();
        clear_unmapped_live_values(&result, &mut w);
        apply_values_to_asset(&result, &mut w, &mut initialized_min_max);
    }

    // Spawn a dedicated thread for sampling so init_live_state can be called before
    // the Actix runtime starts. The thread performs blocking IO and updates the snapshot.
    std::thread::spawn(move || {
        let mut initialized_min_max = initialized_min_max;
        let mut sample_cache = sample_cache;
        loop {
            let cfg_path = config_path.clone();
            let result = load_config_from_file(&cfg_path)
                .map(|mut cfg| sample_mapping_values(&mut cfg, &mut sample_cache))
                .unwrap_or_default();

            {
                let mut raw_w = raw_clone.write().unwrap();
                *raw_w = serde_json::Value::Object(result.clone());
            }

            {
                let mut w = rendered_clone.write().unwrap();
                clear_unmapped_live_values(&result, &mut w);
                apply_values_to_asset(&result, &mut w, &mut initialized_min_max);
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

    (live_state, stop_flag)
}

fn extract_from_sensors_json(sensors_json: &Value, chip: &str, key: &str) -> Option<f64> {
    let mut value = sensors_json.get(chip)?;
    for segment in key.split('/') {
        value = value.get(segment)?;
    }

    if let Some(s) = value.as_str() {
        return s
            .split_whitespace()
            .next()
            .unwrap_or("")
            .replace(',', ".")
            .parse::<f64>()
            .ok();
    }

    if value.is_number() {
        return value.as_f64();
    }

    None
}

fn normalize_sysfs_value(v: f64, sensor_type: &str, path: &str) -> f64 {
    match sysfs_value_family(path)
        .as_ref()
        .map(|(family, _)| family.as_str())
    {
        Some("temp") | Some("in") | Some("curr") => v / 1000.0,
        Some("power") | Some("freq") => v / 1_000_000.0,
        Some("pwm") if matches!(sensor_type_key(sensor_type).as_str(), "control" | "level") => {
            (v / 255.0) * 100.0
        }
        _ => v,
    }
}

fn normalize_sensors_json_value(v: f64, sensor_type: &str, key: &str) -> f64 {
    match sysfs_value_family(key)
        .as_ref()
        .map(|(family, _)| family.as_str())
    {
        Some("freq") => v / 1_000_000.0,
        Some("pwm") if matches!(sensor_type_key(sensor_type).as_str(), "control" | "level") => {
            if v > 100.0 {
                (v / 255.0) * 100.0
            } else {
                v
            }
        }
        _ => v,
    }
}

pub fn format_sensor_value(v: f64, sensor_type: &str) -> String {
    // Match the human-readable formatting used in the bundled asset.
    match sensor_type.to_lowercase().as_str() {
        t if t.contains("temperature") => format!("{:.2} °C", v),
        t if t.contains("voltage") => format!("{:.3} V", v),
        t if t.contains("power") => format!("{:.1} W", v),
        t if t.contains("fan") => format!("{:.0} RPM", v),
        t if t.contains("load") || t.contains("control") || t.contains("level") => {
            format!("{:.1} %", v)
        }
        t if t.contains("current") => format!("{:.2} A", v),
        t if t.contains("clock") => format!("{:.1} MHz", v),
        t if t.contains("data") && !t.contains("smalldata") => format!("{:.1} GB", v),
        t if t.contains("smalldata") => format!("{:.1} MB", v),
        t if t.contains("timing") => format!("{:.1} ns", v),
        t if t.contains("throughput") => format!("{:.1} KB/s", v),
        _ => format!("{:.2}", v),
    }
}

fn split_template_value(template: &str) -> (usize, Option<String>) {
    let trimmed = template.trim();
    if trimmed.is_empty() || trimmed == "-" {
        return (0, None);
    }

    let mut parts = trimmed.split_whitespace();
    let number = parts.next().unwrap_or("");
    let unit = parts.next().map(str::to_string);
    let decimals = number
        .split(['.', ','])
        .nth(1)
        .map(|fraction| fraction.len())
        .unwrap_or(0);
    (decimals, unit)
}

fn format_like_template(v: f64, sensor_type: &str, template: Option<&str>) -> String {
    if let Some(template) = template {
        let (decimals, unit) = split_template_value(template);
        if let Some(unit) = unit {
            let number = format!("{:.*}", decimals, v);
            return format!("{} {}", number, unit);
        }
    }

    format_sensor_value(v, sensor_type)
}

// Build a Value object matching the OHM asset's case-sensitive schema.
// Always uses TitleCase keys: Value, RawValue, Text, Type.
fn build_value_json(
    value_opt: Option<f64>,
    raw_value_opt: Option<String>,
    text: &str,
    sensor_type: &str,
) -> Value {
    match value_opt {
        Some(v) => {
            let formatted = format_sensor_value(v, sensor_type);
            let raw = raw_value_opt.unwrap_or_else(|| v.to_string());
            json!({"Value": formatted, "RawValue": raw, "Text": text, "Type": sensor_type})
        }
        None => match raw_value_opt {
            Some(raw) => {
                json!({"Value": raw.clone(), "RawValue": raw, "Text": text, "Type": sensor_type})
            }
            None => Value::Null,
        },
    }
}

fn parse_sensor_number(s: &str) -> Option<f64> {
    s.split_whitespace()
        .next()
        .unwrap_or("")
        .replace(',', ".")
        .parse::<f64>()
        .ok()
}

// Load OHM asset: prefer a local `ohm.json` in the current working directory when present
// for integration/testing; otherwise fall back to the bundled asset.
fn load_ohm_asset() -> Value {
    let path = std::path::Path::new("ohm.json");
    if path.exists() {
        if let Ok(s) = std::fs::read_to_string(path) {
            if let Ok(v) = serde_json::from_str(&s) {
                return v;
            }
        }
    }
    serde_json::from_str(OHM_ASSET).unwrap_or(Value::Null)
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

fn set_unavailable_sensor_fields(map: &mut serde_json::Map<String, Value>) {
    for key in ["Value", "RawValue", "Min", "Max", "RawMin", "RawMax"] {
        map.insert(key.to_string(), Value::String("-".to_string()));
    }
}

fn clear_unmapped_live_values(sampled: &serde_json::Map<String, Value>, asset: &mut Value) {
    match asset {
        Value::Object(map) => {
            if let Some(sensor_id) = map
                .get("SensorId")
                .and_then(|value| value.as_str())
                .map(str::to_string)
            {
                if !sampled.contains_key(&sensor_id) {
                    set_unavailable_sensor_fields(map);
                }
            }

            if let Some(children) = map.get_mut("Children") {
                if let Some(arr) = children.as_array_mut() {
                    for child in arr.iter_mut() {
                        clear_unmapped_live_values(sampled, child);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                clear_unmapped_live_values(sampled, item);
            }
        }
        _ => {}
    }
}

fn is_sysfs_value_file(file_name: &str) -> bool {
    file_name.ends_with("_input")
        || file_name.ends_with("_average")
        || (file_name.starts_with("pwm") && file_name[3..].chars().all(|ch| ch.is_ascii_digit()))
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

fn compatible_sensor_text(sensor_id: &str, sensor_type: &str, fallback_text: &str) -> String {
    let Some((kind, index)) = parse_type_index(sensor_id) else {
        return fallback_text.to_string();
    };
    let Some(index) = index.parse::<usize>().ok() else {
        return fallback_text.to_string();
    };

    // Home Assistant compatibility expects the legacy OHM-style generic labels for this board subtree.
    if sensor_id.starts_with("/lpc/it8688e/0/") {
        match (kind.as_str(), sensor_type_key(sensor_type).as_str()) {
            ("fan", "fan") => return format!("Fan #{}", index + 1),
            ("temperature", "temperature") => return format!("Temperature #{}", index + 1),
            _ => {}
        }
    }

    fallback_text.to_string()
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

// Apply sampled values (a map from SensorId -> object) into the OHM asset structure.
// This walks the asset tree and, when a node with a `SensorId` is found, merges the
// sampled fields into that node so the resulting asset mirrors the bundled format.
fn apply_values_to_asset(
    sampled: &serde_json::Map<String, Value>,
    asset: &mut Value,
    initialized_min_max: &mut HashSet<String>,
) {
    match asset {
        Value::Object(map) => {
            // If this object has a SensorId, attempt to apply sampled value
            if let Some(sensor_id) = map
                .get("SensorId")
                .and_then(|value| value.as_str())
                .map(str::to_string)
            {
                if let Some(sample) = sampled.get(&sensor_id) {
                    if let Some(sample_obj) = sample.as_object() {
                        let current_value = sample_obj
                            .get("Value")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        let current_raw_value = sample_obj
                            .get("RawValue")
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        let current_numeric =
                            current_value.as_deref().and_then(parse_sensor_number);
                        let previous_min_numeric = map
                            .get("Min")
                            .and_then(|v| v.as_str())
                            .and_then(parse_sensor_number);
                        let previous_max_numeric = map
                            .get("Max")
                            .and_then(|v| v.as_str())
                            .and_then(parse_sensor_number);

                        let formatted_value = current_numeric.map(|current| {
                            format_like_template(
                                current,
                                sample_obj
                                    .get("Type")
                                    .and_then(|v| v.as_str())
                                    .or_else(|| map.get("Type").and_then(|v| v.as_str()))
                                    .unwrap_or(""),
                                map.get("Value").and_then(|v| v.as_str()),
                            )
                        });

                        for (k, v) in sample_obj.iter() {
                            map.insert(k.clone(), v.clone());
                        }

                        if let Some(formatted_value) = &formatted_value {
                            map.insert("Value".to_string(), Value::String(formatted_value.clone()));
                        }

                        if let (Some(value), Some(raw_value)) = (&current_value, &current_raw_value)
                        {
                            let display_value = formatted_value.as_ref().unwrap_or(value);
                            if initialized_min_max.insert(sensor_id.clone()) {
                                map.insert("Min".to_string(), Value::String(display_value.clone()));
                                map.insert("Max".to_string(), Value::String(display_value.clone()));
                                map.insert("RawMin".to_string(), Value::String(raw_value.clone()));
                                map.insert("RawMax".to_string(), Value::String(raw_value.clone()));
                            } else if let Some(current) = current_numeric {
                                if previous_min_numeric.is_none_or(|min| current < min) {
                                    map.insert(
                                        "Min".to_string(),
                                        Value::String(display_value.clone()),
                                    );
                                    map.insert(
                                        "RawMin".to_string(),
                                        Value::String(raw_value.clone()),
                                    );
                                }

                                if previous_max_numeric.is_none_or(|max| current > max) {
                                    map.insert(
                                        "Max".to_string(),
                                        Value::String(display_value.clone()),
                                    );
                                    map.insert(
                                        "RawMax".to_string(),
                                        Value::String(raw_value.clone()),
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // Recurse into children if present
            if let Some(children) = map.get_mut("Children") {
                if let Some(arr) = children.as_array_mut() {
                    for child in arr.iter_mut() {
                        apply_values_to_asset(sampled, child, initialized_min_max);
                    }
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                apply_values_to_asset(sampled, item, initialized_min_max);
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
    let mut device_entries = Vec::new();

    if let Ok(entries) = fs::read_dir("/sys/class/hwmon") {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = fs::read_to_string(path.join("name"))
                .ok()
                .map(|value| value.trim().to_string());
            device_entries.push((path, name));
        }
    }

    device_entries.sort_by(|(path_a, _), (path_b, _)| path_a.cmp(path_b));
    let mut rank_by_name: HashMap<String, usize> = HashMap::new();

    for (path, name) in device_entries {
        let Some(name) = name else {
            continue;
        };

        let device_rank = {
            let next = rank_by_name.entry(name.clone()).or_insert(0);
            *next += 1;
            *next
        };

        let mut meta = vec![
            ("name".to_string(), name),
            ("device_rank".to_string(), device_rank.to_string()),
        ];
        if let Ok(files) = fs::read_dir(&path) {
            for f in files.flatten() {
                if let Some(fname) = f.file_name().to_str() {
                    if fname.ends_with("_label") || is_sysfs_value_file(fname) || fname == "name" {
                        if let Ok(content) = fs::read_to_string(f.path()) {
                            meta.push((fname.to_string(), content.trim().to_string()));
                        }
                    }
                }
            }
        }
        if let Ok(files) = fs::read_dir(&path) {
            for f in files.flatten() {
                if let Some(fname) = f.file_name().to_str() {
                    if is_sysfs_value_file(fname) && fs::read_to_string(f.path()).is_ok() {
                        let p = f.path().to_string_lossy().to_string();
                        map.insert(p, meta.clone());
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

fn score_sensors_json_candidate(
    chip: &str,
    path: &[String],
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
) -> Option<usize> {
    if sensor_type_key(sensor_type) == "clock" {
        return None;
    }

    let key_path = path.join("/");
    if !sysfs_path_matches_sensor_type(&key_path, sensor_type) {
        return None;
    }

    let joined = normalize(&path.join(" "));
    let text_norm = normalize(text);
    let token_hits = semantic_tokens(text)
        .iter()
        .filter(|token| !token.is_empty() && joined.contains(*token))
        .count();
    let alias_hits = label_aliases(sensor_id, text, sensor_type)
        .iter()
        .filter(|alias| joined.contains(**alias))
        .count();
    let contains_text = !text_norm.is_empty() && joined.contains(&text_norm);
    let mut score = 0usize;

    if contains_text {
        score += 40;
    }
    if token_hits > 0 {
        score += token_hits * 8;
    }
    if alias_hits > 0 {
        score += alias_hits * 20;
    }

    score += sensor_device_hint_score(chip, &[("name".to_string(), chip.to_string())]);

    if let (Some(expected_channel), Some((_, actual_channel))) = (
        expected_sysfs_channel(sensor_id, sensor_type),
        sysfs_value_family(&key_path),
    ) {
        if expected_channel == actual_channel {
            score += 30;
        }
    }

    if matches!(
        sensor_type_key(sensor_type).as_str(),
        "clock" | "power" | "load" | "throughput" | "data" | "smalldata" | "timing" | "factor"
    ) && !contains_text
        && token_hits == 0
        && alias_hits == 0
    {
        return None;
    }

    Some(score)
}

fn search_sensors_json_candidates(
    chip: &str,
    value: &Value,
    path: &mut Vec<String>,
    text: &str,
    sensor_id: &str,
    sensor_type: &str,
    best: &mut Option<(usize, String, String)>,
) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                path.push(key.clone());
                search_sensors_json_candidates(
                    chip,
                    nested,
                    path,
                    text,
                    sensor_id,
                    sensor_type,
                    best,
                );
                path.pop();
            }
        }
        Value::Number(_) | Value::String(_) => {
            if let Some(mut score) =
                score_sensors_json_candidate(chip, path, sensor_id, text, sensor_type)
            {
                if let (Some(expected_channel), Some((_, actual_channel))) = (
                    expected_sysfs_channel(sensor_id, sensor_type),
                    sysfs_value_family(&path.join("/")),
                ) {
                    if expected_channel == actual_channel {
                        score += 20;
                    }
                }

                let key_path = path.join("/");
                match best {
                    Some((best_score, best_chip, best_key))
                        if *best_score > score
                            || (*best_score == score
                                && (best_chip.as_str(), best_key.as_str())
                                    <= (chip, key_path.as_str())) => {}
                    _ => *best = Some((score, chip.to_string(), key_path)),
                }
            }
        }
        _ => {}
    }
}

fn find_in_sensors_json(
    sensors_json: &Value,
    sensor_id: &str,
    text: &str,
    sensor_type: &str,
) -> Option<(String, String)> {
    let obj = sensors_json.as_object()?;
    let mut best: Option<(usize, String, String)> = None;

    for (chip, values) in obj {
        search_sensors_json_candidates(
            chip,
            values,
            &mut Vec::new(),
            text,
            sensor_id,
            sensor_type,
            &mut best,
        );
    }

    best.map(|(_, chip, key)| (chip, key))
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
                "temp2": {
                    "temp2_input": 42.0
                },
                "fan1": {
                    "fan1_input": 1200.0
                }
            },
            "coretemp-isa-0000": {
                "Package id 0": {
                    "temp1_input": 55.0
                }
            }
        });

        // match by key label
        let found =
            find_in_sensors_json(&sensors_json, "/test/temperature/1", "temp2", "Temperature");
        assert!(found.is_some());
        let (chip, key) = found.unwrap();
        assert_eq!(chip, "it8688-isa-0a20");
        assert_eq!(key, "temp2/temp2_input");

        let found2 = find_in_sensors_json(
            &sensors_json,
            "/test/temperature/0",
            "Package",
            "Temperature",
        );
        assert!(found2.is_some());
        let (chip2, key2) = found2.unwrap();
        assert_eq!(chip2, "coretemp-isa-0000");
        assert_eq!(key2, "Package id 0/temp1_input");
    }

    #[test]
    fn test_sysfs_path_matches_sensor_type_rejects_incompatible_units() {
        assert!(sysfs_path_matches_sensor_type(
            "/sys/class/hwmon/hwmon4/fan1_input",
            "Fan"
        ));
        assert!(!sysfs_path_matches_sensor_type(
            "/sys/class/hwmon/hwmon4/freq2_input",
            "Fan"
        ));
        assert!(sysfs_path_matches_sensor_type(
            "/sys/class/hwmon/hwmon4/freq2_input",
            "Clock"
        ));
    }

    #[test]
    fn test_select_sysfs_source_prefers_exact_fan_family_and_device_hint() {
        let mut sysfs = HashMap::new();
        sysfs.insert(
            "/sys/class/hwmon/hwmon4/freq2_input".to_string(),
            vec![("name".to_string(), "amdgpu".to_string())],
        );
        sysfs.insert(
            "/sys/class/hwmon/hwmon4/fan1_input".to_string(),
            vec![("name".to_string(), "amdgpu".to_string())],
        );
        sysfs.insert(
            "/sys/class/hwmon/hwmon8/fan1_input".to_string(),
            vec![("name".to_string(), "octo".to_string())],
        );

        let selected = select_sysfs_source("/gpu-amd/0/fan/0", "GPU Fan", "Fan", &sysfs)
            .expect("should select a compatible fan source");
        assert_eq!(
            selected.path.as_deref(),
            Some("/sys/class/hwmon/hwmon4/fan1_input")
        );
    }

    #[test]
    fn test_select_sysfs_source_rejects_numeric_only_power_matches() {
        let mut sysfs = HashMap::new();
        sysfs.insert(
            "/sys/class/hwmon/hwmon8/power1_input".to_string(),
            vec![
                ("name".to_string(), "octo".to_string()),
                ("power1_label".to_string(), "Fan 1 power".to_string()),
            ],
        );
        sysfs.insert(
            "/sys/class/hwmon/hwmon4/power1_average".to_string(),
            vec![
                ("name".to_string(), "amdgpu".to_string()),
                ("power1_label".to_string(), "PPT".to_string()),
            ],
        );

        assert!(
            select_sysfs_source("/amdcpu/0/power/1", "Core #1 (SMU)", "Power", &sysfs)
                .is_none(),
            "CPU core power should not latch onto unrelated fan power labels just because they share a channel number"
        );
    }

    #[test]
    fn test_select_sysfs_source_maps_gpu_package_power_to_ppt() {
        let mut sysfs = HashMap::new();
        sysfs.insert(
            "/sys/class/hwmon/hwmon8/power1_input".to_string(),
            vec![
                ("name".to_string(), "octo".to_string()),
                ("power1_label".to_string(), "Fan 1 power".to_string()),
            ],
        );
        sysfs.insert(
            "/sys/class/hwmon/hwmon4/power1_average".to_string(),
            vec![
                ("name".to_string(), "amdgpu".to_string()),
                ("power1_label".to_string(), "PPT".to_string()),
            ],
        );

        let selected = select_sysfs_source("/gpu-amd/0/power/3", "GPU Package", "Power", &sysfs)
            .expect("GPU package power should resolve to the amdgpu PPT reading");
        assert_eq!(
            selected.path.as_deref(),
            Some("/sys/class/hwmon/hwmon4/power1_average")
        );
    }

    #[test]
    fn test_select_powercap_source_maps_cpu_package_to_rapl_package() {
        let mut powercap = HashMap::new();
        powercap.insert(
            "/sys/class/powercap/intel-rapl:0/energy_uj".to_string(),
            vec![("name".to_string(), "package-0".to_string())],
        );
        powercap.insert(
            "/sys/class/powercap/intel-rapl:0:0/energy_uj".to_string(),
            vec![("name".to_string(), "core".to_string())],
        );

        let selected = select_powercap_source("/amdcpu/0/power/0", "Package", "Power", &powercap)
            .expect("CPU package power should resolve to the package RAPL domain");
        assert_eq!(
            selected.path.as_deref(),
            Some("/sys/class/powercap/intel-rapl:0/energy_uj")
        );
        assert!(
            select_powercap_source("/amdcpu/0/power/1", "Core #1 (SMU)", "Power", &powercap,)
                .is_none()
        );
    }

    #[test]
    fn test_compute_powercap_watts_handles_counter_wrap() {
        let watts = compute_powercap_watts(900, 100, Duration::from_secs(2), Some(1_000))
            .expect("counter wrap with a max range should be handled");
        assert_eq!(watts, 0.0001);
    }

    #[test]
    fn test_select_sysfs_source_uses_lpc_device_rank_to_avoid_duplicate_fan_banks() {
        let mut sysfs = HashMap::new();
        sysfs.insert(
            "/sys/class/hwmon/hwmon5/fan1_input".to_string(),
            vec![
                ("name".to_string(), "it8628".to_string()),
                ("device_rank".to_string(), "1".to_string()),
            ],
        );
        sysfs.insert(
            "/sys/class/hwmon/hwmon6/fan1_input".to_string(),
            vec![
                ("name".to_string(), "it8628".to_string()),
                ("device_rank".to_string(), "2".to_string()),
            ],
        );

        let first = select_sysfs_source("/lpc/it8688e/0/fan/0", "CPU Fan", "Fan", &sysfs)
            .expect("first lpc chip should resolve");
        assert_eq!(
            first.path.as_deref(),
            Some("/sys/class/hwmon/hwmon5/fan1_input")
        );

        let second = select_sysfs_source(
            "/lpc/it8792e/0/fan/0",
            "System Fan #5 / Pump",
            "Fan",
            &sysfs,
        )
        .expect("second lpc chip should resolve to the second IT8628 device");
        assert_eq!(
            second.path.as_deref(),
            Some("/sys/class/hwmon/hwmon6/fan1_input")
        );
    }

    #[test]
    fn test_select_proc_cpuinfo_source_maps_cpu_clock_entries() {
        let avg = select_proc_cpuinfo_source("/amdcpu/0/clock/1", "Cores (Average)", "Clock")
            .expect("average CPU clock should use /proc/cpuinfo");
        assert_eq!(avg.kind, "proc_cpuinfo");
        assert_eq!(avg.key.as_deref(), Some("core_average"));

        let core = select_proc_cpuinfo_source("/amdcpu/0/clock/3", "Core #1", "Clock")
            .expect("per-core CPU clock should use /proc/cpuinfo");
        assert_eq!(core.key.as_deref(), Some("core:0"));
    }

    #[test]
    fn test_parse_proc_cpuinfo_sample_aggregates_sibling_threads_by_core() {
        let sample = parse_proc_cpuinfo_sample(
            "processor\t: 0\ncore id\t\t: 0\ncpu MHz\t\t: 3600.0\n\nprocessor\t: 8\ncore id\t\t: 0\ncpu MHz\t\t: 4200.0\n\nprocessor\t: 1\ncore id\t\t: 1\ncpu MHz\t\t: 3500.0\n"
        )
        .expect("cpuinfo sample should parse");
        assert_eq!(sample.core_mhz.get(&0).copied(), Some(4200.0));
        assert_eq!(sample.core_mhz.get(&1).copied(), Some(3500.0));
        assert_eq!(sample.average_mhz, 3850.0);
    }

    #[test]
    fn test_compatible_sensor_text_uses_legacy_ohm_labels_for_it8688e_board_sensors() {
        assert_eq!(
            compatible_sensor_text("/lpc/it8688e/0/fan/0", "Fan", "CPU Fan"),
            "Fan #1"
        );
        assert_eq!(
            compatible_sensor_text("/lpc/it8688e/0/fan/4", "Fan", "CPU Optional Fan"),
            "Fan #5"
        );
        assert_eq!(
            compatible_sensor_text("/lpc/it8688e/0/temperature/2", "Temperature", "CPU"),
            "Temperature #3"
        );
        assert_eq!(
            compatible_sensor_text("/gpu-amd/0/temperature/0", "Temperature", "GPU Core"),
            "GPU Core"
        );
    }

    #[test]
    fn test_select_sysfs_source_only_maps_gpu_clocks_when_label_and_channel_match() {
        let mut sysfs = HashMap::new();
        let gpu_meta = vec![
            ("name".to_string(), "amdgpu".to_string()),
            ("freq1_label".to_string(), "sclk".to_string()),
            ("freq2_label".to_string(), "mclk".to_string()),
        ];
        sysfs.insert(
            "/sys/class/hwmon/hwmon4/freq1_input".to_string(),
            gpu_meta.clone(),
        );
        sysfs.insert("/sys/class/hwmon/hwmon4/freq2_input".to_string(), gpu_meta);

        let gpu_core = select_sysfs_source("/gpu-amd/0/clock/0", "GPU Core", "Clock", &sysfs)
            .expect("GPU core clock should resolve");
        assert_eq!(
            gpu_core.path.as_deref(),
            Some("/sys/class/hwmon/hwmon4/freq1_input")
        );

        let gpu_memory = select_sysfs_source("/gpu-amd/0/clock/2", "GPU Memory", "Clock", &sysfs)
            .expect("GPU memory clock should resolve");
        assert_eq!(
            gpu_memory.path.as_deref(),
            Some("/sys/class/hwmon/hwmon4/freq2_input")
        );

        assert!(
            select_sysfs_source("/amdcpu/0/clock/3", "Core #1", "Clock", &sysfs).is_none(),
            "CPU clocks should be left on the bundled OHM values when no reliable source exists"
        );
    }

    #[test]
    fn test_normalize_sysfs_value_applies_type_specific_scaling() {
        assert_eq!(
            normalize_sysfs_value(
                1_249_000_000.0,
                "Clock",
                "/sys/class/hwmon/hwmon4/freq2_input"
            ),
            1249.0
        );
        assert_eq!(
            normalize_sysfs_value(117.0, "Control", "/sys/class/hwmon/hwmon5/pwm1"),
            (117.0 / 255.0) * 100.0
        );
    }

    #[test]
    fn test_reconcile_mapping_config_drops_incompatible_sources() {
        let mut cfg = MappingConfig {
            mappings: vec![MappingEntry {
                ohm: "/gpu-amd/0/fan/0".to_string(),
                text: "GPU Fan".to_string(),
                sensor_type: "Fan".to_string(),
                source: SensorSource {
                    kind: "sysfs".to_string(),
                    path: Some("/sys/class/hwmon/hwmon4/freq2_input".to_string()),
                    chip: None,
                    key: None,
                },
            }],
        };

        reconcile_mapping_config(&mut cfg, &HashMap::new(), &HashMap::new(), &Value::Null);
        assert!(cfg.mappings.is_empty());
    }

    #[test]
    fn test_build_value_json_keys_and_format() {
        // numeric value path
        let v = build_value_json(Some(4.02), Some("4020".to_string()), "Vcore", "Voltage");
        // keys should be TitleCase and present
        assert!(v.get("Value").is_some());
        assert!(v.get("RawValue").is_some());
        assert_eq!(v.get("Text").and_then(|s| s.as_str()), Some("Vcore"));
        assert_eq!(v.get("Type").and_then(|s| s.as_str()), Some("Voltage"));
        // lowercase variants must not be present
        assert!(v.get("value").is_none());
        assert!(v.get("rawvalue").is_none());
        assert!(v.get("text").is_none());
        assert!(v.get("type").is_none());

        // raw string path
        let v2 = build_value_json(None, Some("N/A".to_string()), "NoVal", "Unknown");
        assert!(v2.get("Value").is_some());
        assert_eq!(v2.get("Value").and_then(|s| s.as_str()), Some("N/A"));
    }

    #[test]
    fn test_apply_values_to_asset_case_sensitive() {
        let mut asset = json!({
            "Children": [
                {"SensorId": "/test/1"}
            ]
        });
        let mut initialized_min_max = HashSet::new();

        let mut sampled = serde_json::Map::new();
        sampled.insert(
            "/test/1".to_string(),
            json!({"Value": "42", "RawValue": "42", "Text": "Test", "Type": "Voltage"}),
        );

        apply_values_to_asset(&sampled, &mut asset, &mut initialized_min_max);
        let child = &asset["Children"][0];
        // TitleCase keys present
        assert!(child.get("Value").is_some());
        assert!(child.get("RawValue").is_some());
        assert!(child.get("Text").is_some());
        assert!(child.get("Type").is_some());
        // Lowercase keys should not be present
        assert!(child.get("value").is_none());
        assert!(child.get("rawvalue").is_none());
        assert!(child.get("text").is_none());
        assert!(child.get("type").is_none());
    }

    #[test]
    fn test_clear_unmapped_live_values_marks_stale_entries_unavailable() {
        let mut asset = json!({
            "Children": [
                {
                    "SensorId": "/test/mapped",
                    "Value": "42.00 °C",
                    "RawValue": "42000",
                    "Min": "42.00 °C",
                    "Max": "42.00 °C",
                    "RawMin": "42000",
                    "RawMax": "42000"
                },
                {
                    "SensorId": "/test/stale",
                    "Value": "61.4 W",
                    "RawValue": "61.4 W",
                    "Min": "51.5 W",
                    "Max": "83.4 W",
                    "RawMin": "51.5 W",
                    "RawMax": "83.4 W"
                }
            ]
        });
        let mut sampled = serde_json::Map::new();
        sampled.insert(
            "/test/mapped".to_string(),
            json!({"Value": "42.00 °C", "RawValue": "42000", "Text": "Mapped", "Type": "Temperature"}),
        );

        clear_unmapped_live_values(&sampled, &mut asset);

        let stale = &asset["Children"][1];
        for key in ["Value", "RawValue", "Min", "Max", "RawMin", "RawMax"] {
            assert_eq!(stale.get(key).and_then(|v| v.as_str()), Some("-"));
        }

        let mapped = &asset["Children"][0];
        assert_eq!(
            mapped.get("Value").and_then(|v| v.as_str()),
            Some("42.00 °C")
        );
        assert_eq!(
            mapped.get("RawValue").and_then(|v| v.as_str()),
            Some("42000")
        );
    }

    #[test]
    fn test_apply_values_to_asset_tracks_runtime_min_max() {
        let mut asset = json!({
            "Children": [
                {
                    "SensorId": "/test/1",
                    "Min": "1.00 V",
                    "Max": "9.00 V",
                    "RawMin": "1.00 V",
                    "RawMax": "9.00 V"
                }
            ]
        });
        let mut initialized_min_max = HashSet::new();

        let mut first = serde_json::Map::new();
        first.insert(
            "/test/1".to_string(),
            json!({"Value": "4.000 V", "RawValue": "4000", "Text": "Test", "Type": "Voltage"}),
        );
        apply_values_to_asset(&first, &mut asset, &mut initialized_min_max);

        let child = &asset["Children"][0];
        assert_eq!(child.get("Min").and_then(|v| v.as_str()), Some("4.000 V"));
        assert_eq!(child.get("Max").and_then(|v| v.as_str()), Some("4.000 V"));
        assert_eq!(child.get("RawMin").and_then(|v| v.as_str()), Some("4000"));
        assert_eq!(child.get("RawMax").and_then(|v| v.as_str()), Some("4000"));

        let mut second = serde_json::Map::new();
        second.insert(
            "/test/1".to_string(),
            json!({"Value": "3.500 V", "RawValue": "3500", "Text": "Test", "Type": "Voltage"}),
        );
        apply_values_to_asset(&second, &mut asset, &mut initialized_min_max);

        let child = &asset["Children"][0];
        assert_eq!(child.get("Min").and_then(|v| v.as_str()), Some("3.500 V"));
        assert_eq!(child.get("Max").and_then(|v| v.as_str()), Some("4.000 V"));
        assert_eq!(child.get("RawMin").and_then(|v| v.as_str()), Some("3500"));
        assert_eq!(child.get("RawMax").and_then(|v| v.as_str()), Some("4000"));
    }
}
