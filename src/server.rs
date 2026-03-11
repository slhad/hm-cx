// This file sets up the web server using the Actix-web framework and exports a function to start the server.

use crate::routes::init_routes;
use actix_web::{web, App, HttpServer};
use std::sync::Arc;
use tokio::sync::Notify;

/// Start the server and optionally inject a shared live snapshot into the App data
pub async fn start_server(
    port: u16,
    snapshot: Option<std::sync::Arc<std::sync::RwLock<serde_json::Value>>>,
    shutdown_notify: Option<Arc<Notify>>,
) -> std::io::Result<()> {
    // Move the optional snapshot into the server factory closure so each App gets access
    HttpServer::new(move || {
        let mut app = App::new();
        if let Some(ref s) = snapshot {
            app = app.app_data(web::Data::from(s.clone()));
        }
        app.configure(init_routes)
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await
    .inspect(|res| {
        // When the server stops running, notify the sampler to shut down if provided.
        if let Some(n) = shutdown_notify {
            n.notify_waiters();
        }
        let _ = res;
    })
}
