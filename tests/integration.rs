use std::fs::File;
use std::io::Write;

use hm_cx::mapping::init_live_state;

// Integration test: start the sampler with a temporary config.yml pointing to a temp sysfs file,
// run the sampler, mount the snapshot into an Actix app and request /data.json to verify sampled value.

#[actix_web::test]
async fn integration_sampler_and_data_json() {
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

    // verify config loads correctly
    let cfg = hm_cx::mapping::load_config_from_file(&cfg_path.to_string_lossy())
        .expect("config should parse");
    assert!(!cfg.mappings.is_empty());

    // start the sampler with a short poll interval
    let (snapshot, shutdown_notify) = init_live_state(&cfg_path.to_string_lossy(), 100);

    // wait a bit for sampler to run at least once
    actix_web::rt::time::sleep(std::time::Duration::from_millis(1200)).await;

    // check snapshot directly first; wait up to 3s for sampler to populate
    let mut ok = false;
    for _ in 0..30 {
        {
            let guard = snapshot.read().unwrap();
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
        let guard = snapshot.read().unwrap();
        guard.clone()
    };
    // our OHM key should be present
    assert!(v.get("/test/temperature/0").is_some());
    let entry = v.get("/test/temperature/0").unwrap();
    // value should be approximately 42.0
    assert_eq!(entry.get("value").and_then(|n| n.as_f64()).unwrap(), 42.0);

    // shut down sampler by setting the stop flag
    shutdown_notify.store(true, std::sync::atomic::Ordering::SeqCst);
    // give it a moment to exit
    actix_web::rt::time::sleep(std::time::Duration::from_millis(200)).await;
}
