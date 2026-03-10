// This file sets up the web server using the Actix-web framework and exports a function to start the server.

use actix_web::{App, HttpServer};
use crate::routes::init_routes;

pub async fn start_server(port: u16) -> std::io::Result<()> {
    HttpServer::new(|| {
        App::new()
            .configure(init_routes)
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await
}