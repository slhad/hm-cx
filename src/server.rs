// This file sets up the web server using the Actix-web framework and exports a function to start the server.

use crate::routes::init_routes;
use actix_web::{web, App, HttpServer};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Start the server and optionally inject a shared live snapshot into the App data
pub async fn start_server(
    port: u16,
    snapshot: Option<std::sync::Arc<std::sync::RwLock<serde_json::Value>>>,
    shutdown_notify: Option<Arc<AtomicBool>>,
) -> std::io::Result<()> {
    // Move the optional snapshot into the server factory closure so each App gets access
    HttpServer::new(move || {
        let mut app = App::new();
        if let Some(ref s) = snapshot {
            // Use Data::new to ensure the extractor type matches what handlers expect
            app = app.app_data(web::Data::new(s.clone()));
        }
        app.configure(init_routes)
    })
    .bind(format!("0.0.0.0:{}", port))?
    .run()
    .await
    .inspect(|res| {
        // When the server stops running, set the stop flag so the sampler thread exits.
        if let Some(n) = shutdown_notify {
            n.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        let _ = res;
    })
}
