use std::fs::File;
use std::io::Write;
use std::sync::OnceLock;

use hm_cx::mapping::init_live_state;
use tokio::sync::Mutex;

fn cwd_mutex() -> &'static Mutex<()> {
    static CWD_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
    CWD_MUTEX.get_or_init(|| Mutex::new(()))
}

// Integration test: start the sampler with a temporary config.yml pointing to a temp sysfs file,
// run the sampler, mount the snapshot into an Actix app and request /data.json to verify sampled value.

#[actix_web::test]
async fn integration_sampler_and_data_json() {
    let _cwd_lock = cwd_mutex().lock().await;
    // create a tempdir for config and fake sysfs
    let td = tempfile::tempdir().expect("tempdir");
    let sysfs_file_path = td.path().join("fake_temp_input");
    let mut f = File::create(&sysfs_file_path).expect("create sysfs file");
    // write a millidegree-like value that the sampler will normalize (42000 -> 42.0)
    writeln!(f, "42000").expect("write value");

    // create a config.yml in the tempdir
    let cfg_path = td.path().join("config.yml");
    let cfg_contents = format!(
        "mappings:\n  - ohm: /test/temperature/0\n    text: Test Temp\n    type: Temperature\n    source:\n      kind: sysfs\n      path: {}\n",
        sysfs_file_path.to_string_lossy()
    );
    std::fs::write(&cfg_path, cfg_contents).expect("write config.yml");

    // create a minimal ohm.json in the tempdir so the sampler can apply values into the asset
    let ohm_asset = serde_json::json!({
        "Children": [
            {
                "SensorId": "/test/temperature/0",
                "Text": "Test Temp",
                "Type": "Temperature",
                "Children": []
            }
        ]
    });
    let ohm_path = td.path().join("ohm.json");
    std::fs::write(&ohm_path, serde_json::to_string(&ohm_asset).unwrap()).expect("write ohm.json");

    // switch current dir to tempdir so init_live_state will load the local ohm.json
    let orig_cwd = std::env::current_dir().expect("cwd");
    std::env::set_current_dir(td.path()).expect("set cwd");

    // verify config loads correctly
    let cfg = hm_cx::mapping::load_config_from_file(&cfg_path.to_string_lossy())
        .expect("config should parse");
    assert!(!cfg.mappings.is_empty());

    // start the sampler with a short poll interval
    let (live_state, shutdown_notify) = init_live_state(&cfg_path.to_string_lossy(), 100);

    // wait a bit for sampler to run at least once
    actix_web::rt::time::sleep(std::time::Duration::from_millis(1200)).await;

    // check snapshot directly first; wait up to 3s for sampler to populate
    let mut ok = false;
    for _ in 0..30 {
        {
            let guard = live_state.rendered.read().unwrap();
            if guard.is_object() && !guard.as_object().unwrap().is_empty() {
                ok = true;
                break;
            }
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    if !ok {
        // gather debug info
        let cfg_text = std::fs::read_to_string(&cfg_path).unwrap_or_else(|_| "<no cfg>".into());
        let file_text =
            std::fs::read_to_string(&sysfs_file_path).unwrap_or_else(|_| "<no file>".into());
        panic!(
            "snapshot did not populate in time. cfg:\n{}\nfile:\n{}",
            cfg_text, file_text
        );
    }
    // debug print snapshot content
    let v = {
        let guard = live_state.rendered.read().unwrap();
        guard.clone()
    };

    // find the sensor node by SensorId inside the OHM-shaped asset
    fn find_sensor_node<'a>(
        node: &'a serde_json::Value,
        id: &str,
    ) -> Option<&'a serde_json::Value> {
        if let Some(obj) = node.as_object() {
            if let Some(serde_json::Value::String(sid)) = obj.get("SensorId") {
                if sid == id {
                    return Some(node);
                }
            }
            if let Some(children) = obj.get("Children").and_then(|c| c.as_array()) {
                for child in children {
                    if let Some(found) = find_sensor_node(child, id) {
                        return Some(found);
                    }
                }
            }
        } else if let Some(arr) = node.as_array() {
            for item in arr {
                if let Some(found) = find_sensor_node(item, id) {
                    return Some(found);
                }
            }
        }
        None
    }

    if let Some(entry) = find_sensor_node(&v, "/test/temperature/0") {
        // Found in OHM-shaped asset: Value is a formatted string, RawValue is raw sysfs string
        assert_eq!(
            entry.get("Value").and_then(|s| s.as_str()),
            Some("42.00 °C")
        );
        assert_eq!(
            entry.get("RawValue").and_then(|s| s.as_str()),
            Some("42000")
        );
        // Enforce TitleCase-only: lowercase 'value' must not be present
        assert!(entry.get("value").is_none());
    } else {
        panic!("sensor node should exist in OHM-shaped asset with TitleCase sampled fields");
    }

    let raw_v = {
        let guard = live_state.raw.read().unwrap();
        guard.clone()
    };
    assert_eq!(
        raw_v
            .get("/test/temperature/0")
            .and_then(|v| v.get("RawValue"))
            .and_then(|s| s.as_str()),
        Some("42000")
    );

    // shut down sampler by setting the stop flag
    shutdown_notify.store(true, std::sync::atomic::Ordering::SeqCst);
    // give it a moment to exit
    actix_web::rt::time::sleep(std::time::Duration::from_millis(200)).await;

    // restore cwd
    std::env::set_current_dir(orig_cwd).expect("restore cwd");
}
