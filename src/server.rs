// This file sets up the web server using the Actix-web framework and exports a function to start the server.

use crate::mapping::LiveState;
use crate::routes::init_routes;
use actix_web::{web, App, HttpServer};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Start the server and optionally inject the shared live state into the App data.
pub async fn start_server(
    port: u16,
    live_state: Option<Arc<LiveState>>,
    shutdown_notify: Option<Arc<AtomicBool>>,
) -> std::io::Result<()> {
    // Move the optional live state into the server factory closure so each App gets access.
    HttpServer::new(move || {
        let mut app = App::new();
        if let Some(ref state) = live_state {
            // Use Data::new to ensure the extractor type matches what handlers expect
            app = app.app_data(web::Data::new(state.clone()));
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
