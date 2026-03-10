// Entry point: initialize configuration and start the server.

mod config;
mod server;
mod routes;
mod handlers;
mod utils;

use std::env;
use config::Config;
use server::start_server;

fn main() {
    // Read optional PORT env var and build config
    let port = env::var("PORT").ok().and_then(|s| s.parse::<u16>().ok());
    let config = Config::new(port);

    // Start the async server runtime and run the server
    if let Err(err) = actix_web::rt::System::new().block_on(start_server(config.port)) {
        eprintln!("Server failed: {}", err);
        std::process::exit(1);
    }
}