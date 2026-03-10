# Rust Web Server

This project is a simple web server built in Rust that listens on port 8080 by default but allows for configurable ports. It is designed to demonstrate the use of Rust for web development, utilizing the Actix-web or Rocket framework.

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

   By default, the server will listen on port 8080. You can configure the port by modifying the configuration file or passing it as an environment variable.

## Usage

Once the server is running, you can access it by navigating to `http://localhost:8080` in your web browser. The server will respond to requests based on the defined routes and handlers.

## Contributing

Contributions are welcome! Please feel free to submit a pull request or open an issue for any enhancements or bug fixes.

## License

This project is licensed under the MIT License. See the LICENSE file for more details.