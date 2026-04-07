use hm_cx::cli::{parse_command, run_command};
use hm_cx::config::Config;
use hm_cx::mapping::{self, LiveState};
use hm_cx::server::start_server;
use std::env;
use std::sync::Arc;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    match parse_command(&args) {
        Ok(Some(command)) => {
            if let Err(err) = run_command(command) {
                eprintln!("{err}");
                std::process::exit(1);
            }
            return;
        }
        Ok(None) => {}
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }

    let port = env::var("PORT").ok().and_then(|s| s.parse::<u16>().ok());
    let config = Config::new(port);

    let mut live_state_opt: Option<Arc<LiveState>> = None;
    let mut shutdown_notify: Option<std::sync::Arc<std::sync::atomic::AtomicBool>> = None;
    if std::path::Path::new("config.yml").exists() {
        let (live_state, notify) = mapping::init_live_state("config.yml", 1000);
        live_state_opt = Some(live_state);
        shutdown_notify = Some(notify);
    }

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).init();
    if let Err(err) = actix_web::rt::System::new().block_on(start_server(
        config.port,
        live_state_opt,
        shutdown_notify,
    )) {
        eprintln!("Server failed: {err}");
        std::process::exit(1);
    }
}
