
# hm-cx — OpenHardwareMonitor-like HTTP server

This project implements a small Rust HTTP server that mimics the OpenHardwareMonitor / LibreHardwareMonitor web server by exposing sensor data in the same JSON structure (the same shape served at http://localhost:8085/data.json). The repository includes a bundled export at `assets/openhardwaremonitor_localhost_8085_data.json` which is an export of LibreHardwareMonitor (an updated fork of OpenHardwareMonitor) and used by tests and local development.

## Project Structure

```
hm-cx
├── src
│   ├── main.rs          # Entry point of the application
│   ├── config.rs        # Configuration structure and loading logic
│   ├── server.rs        # Web server setup
│   ├── routes.rs        # Application routes
│   ├── handlers          # Request handlers
│   │   └── mod.rs
│   └── utils            # Utility functions
│       └── mod.rs
├── Cargo.toml           # Project configuration and dependencies
├── Cargo.lock           # Dependency versions for reproducibility
├── .gitignore           # Files and directories to ignore by Git
└── README.md            # Project documentation
```

## Setup Instructions

1. **Clone the repository:**
   ```
   git clone <repository-url>
   cd hm-cx
   ```

2. **Build the project:**
   ```
   cargo build
   ```

3. **Run the server:**
   ```
   cargo run
   ```

   By default the server listens on port `8080` (configurable via `PORT` env). The server exposes the OpenHardwareMonitor-style JSON at the `/data.json` route so clients written for OHM/LHMonitor can point to this server (the original OHM webserver uses port `8085`).

## Usage

Once the server is running, you can access it by navigating to `http://localhost:8080` in your web browser. The OpenHardwareMonitor-compatible JSON is available at `http://localhost:8080/data.json`.

### Behavior when using `config.yml` (live sampling)

When `config.yml` exists the server starts a background sampler and injects a shared in-memory snapshot into the app. In this mode `/data.json` is authoritative and will return:

- `200 OK` with the live JSON snapshot when the sampler produced at least one sensor value.
- `500 Internal Server Error` with a JSON error payload when the snapshot is empty or `null` (this means your mappings produced no values). The JSON payload looks like:

```json
{
  "error": "no sensor data available",
  "reason": "mappings produced no values",
  "snapshot_size": 0,
  "timestamp_unix": 1670000000,
  "suggestion": "inspect /snapshot.json and verify config.yml mappings"
}
```

Use `/snapshot.json` to inspect the raw in-memory snapshot for debugging.

### Logging and debug

This server uses `tracing` for structured logs. Control verbosity with the `RUST_LOG` environment variable. Examples:

- Run with default (info): `cargo run`
- Run with debug logging: `RUST_LOG=debug cargo run`

Small example

Run the server with debug logging enabled and inspect logs (example):

```bash
RUST_LOG=debug cargo run
```

Example JSON log line you may see when a mapping produces no value:

```json
{
  "timestamp": "2026-03-12T12:34:56Z",
  "level": "WARN",
  "message": "mapping produced no value",
  "ohm": "/test/temperature/0",
  "source": { "kind": "sysfs", "path": "/tmp/fake_temp_input" }
}
```

This illustrates the structured fields emitted by the tracer and helps when shipping logs to aggregators.


## Contributing

Contributions are welcome! Please feel free to submit a pull request or open an issue for any enhancements or bug fixes.

## License

This project is licensed under the MIT License. See the LICENSE file for more details.
