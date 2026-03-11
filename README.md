
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

## Contributing

Contributions are welcome! Please feel free to submit a pull request or open an issue for any enhancements or bug fixes.

## License

This project is licensed under the MIT License. See the LICENSE file for more details.
