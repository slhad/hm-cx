pub fn log_request(request: &str) {
    tracing::info!(request = %request, "Received request");
}

// Add other utility functions here
