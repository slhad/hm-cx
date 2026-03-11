// Entry point: initialize configuration and start the server.

mod config;
mod handlers;
mod mapping;
mod routes;
mod server;
mod utils;

// mapping module is declared above; refer to it as `mapping` directly
use config::Config;
use serde_json::Value;
use server::start_server;
use std::env;
use std::sync::{Arc, RwLock};

fn main() {
    // Support a simple CLI: `generate-config` runs the mapping without starting the webserver.
    if let Some(cmd) = env::args().nth(1) {
        if cmd == "generate-config" {
            match mapping::generate_config() {
                Ok(()) => std::process::exit(0),
                Err(e) => {
                    eprintln!("generate-config failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
    // Read optional PORT env var and build config
    let port = env::var("PORT").ok().and_then(|s| s.parse::<u16>().ok());
    let config = Config::new(port);

    // Start the async server runtime and run the server
    // Initialize live state sampling if config.yml exists and pass the snapshot into the server.
    // mapping::init_live_state now returns (snapshot, shutdown_notify). We keep the notify
    // so the main thread can signal the sampler to stop once the server shuts down.
    let mut snapshot_opt: Option<Arc<RwLock<Value>>> = None;
    let mut shutdown_notify: Option<std::sync::Arc<tokio::sync::Notify>> = None;
    if std::path::Path::new("config.yml").exists() {
        let (snap, notify) = mapping::init_live_state("config.yml", 1000);
        snapshot_opt = Some(snap);
        shutdown_notify = Some(notify);
    }

    // Run server; when it returns (server stopped), the server will notify the sampler.
    if let Err(err) = actix_web::rt::System::new().block_on(start_server(config.port, snapshot_opt, shutdown_notify))
    {
        eprintln!("Server failed: {}", err);
        std::process::exit(1);
    }
}
