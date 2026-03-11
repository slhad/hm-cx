// This file defines the application's routes. It exports a function `init_routes` that sets up the routing for the web server, linking routes to their respective handlers.

use crate::handlers::handle_generate_config;
use crate::handlers::{handle_data_json, handle_index};
use actix_web::web;

pub fn init_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("/").to(handle_index))
        .service(web::resource("/data.json").to(handle_data_json));

    cfg.service(web::resource("/generate-config").route(web::post().to(handle_generate_config)));
}
