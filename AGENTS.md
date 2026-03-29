# Agents Guide — hm-cx (Rust web server)

This document is a concise, actionable guide for agentic coding agents operating in this repository. It covers how to build, lint and test the project (including running a single test), plus the repository's style and behavioral rules agents must follow.

Keep commands local: run `cargo build`, `cargo test`, `cargo fmt`, `cargo clippy` and manual checks before proposing changes. Do not push or force-push to remotes unless explicitly instructed.

---

## Quick Commands

- Build (debug):

```bash
cargo build
```

- Build (release):

```bash
cargo build --release
```

- Run server (default port 8080):

```bash
cargo run
```

- Run server on custom port (example 3000):

```bash
PORT=3000 cargo run
```

- Run the full test suite:

```bash
cargo test
```

- Run a single test (by exact test name):

```bash
# Use the test function name. Example from src/handlers/mod.rs:
cargo test data_json_route_returns_the_bundled_asset -- --exact
```

- Run a single test and see stdout (useful for debugging):

```bash
cargo test data_json_route_returns_the_bundled_asset -- --exact --nocapture
```

- Format code (apply):

```bash
rustup component add rustfmt || true
cargo fmt --all
```

- Format check (CI / pre-merge):

```bash
cargo fmt --all -- --check
```

- Lint with Clippy (recommended flags):

```bash
rustup component add clippy || true
cargo clippy --all-targets --all-features -- -D warnings
```

--

## Running and Testing Notes

- The project is a small Actix-web server. Tests use `#[actix_web::test]` and the `actix_web::test` helpers.
- Tests live inline (unit tests) in modules such as `src/handlers/mod.rs` and may use bundled assets (e.g. `assets/openhardwaremonitor_localhost_8085_data.json`).
- To target tests in a particular file or module, filter by the test function name as shown above. If multiple tests share a prefix you can use a substring to match several tests.
- Compatibility contract for live data: `/data.json` must preserve the OHM-compatible tree shape from `ohm.json`, including hierarchical paths, sensor metadata, and displayed units. Live readings are expected to drift over time, so enforce schema compatibility separately from value equality.
- Schema compatibility endpoint: use `/compare/schema` to validate live `/data.json` against `ohm.json`. This check ignores live value drift, treats `-` in the reference snapshot as "unavailable at capture time", and accepts unit-family scaling for throughput-style metrics (for example `KB/s` vs `MB/s`).
- Live dashboard: `/live` serves an HTML status board that polls `/data.json`, `/health`, and `/compare/schema` in the browser. Prefer this route for quick manual validation of live sensor data, health, and compatibility state.

Example: run all tests with the substring `data_json`:

```bash
cargo test data_json
```

Use `-- --nocapture` to capture printed output from tests.

--

## Code Style Guidelines

Follow Rust community conventions and keep the codebase consistent.

- Formatting
  - Use `rustfmt` for all formatting. Run `cargo fmt --all` before creating patches.
  - Keep default rustfmt settings unless a repo-level `rustfmt.toml` is added. Avoid manual alignments that rustfmt will revert.

- Imports and modules
  - Order imports in broad groups: `std::...`, external crates (e.g. `actix_web`), then `crate::...` or local modules.
  - Avoid glob imports (e.g. `use foo::*;`) except in test modules where brevity helps readability.
  - Prefer explicit, small `use` lists. Example:

```rust
use actix_web::{App, HttpServer};
use crate::routes::init_routes;
```

- Naming
  - Types, enums and traits: `CamelCase` (e.g. `Config`, `MyError`).
  - Functions, variables, modules, and file names: `snake_case` (e.g. `start_server`, `config.rs`).
  - Constants: `SCREAMING_SNAKE_CASE` (e.g. `OPEN_HARDWARE_MONITOR_DATA_JSON`).
  - Tests: function names should describe intent and use underscores (existing style is fine).

- Types & signatures
  - Public APIs (pub structs, functions) should use explicit types in signatures.
  - Keep functions short and focused; extract helpers where behaviour becomes complex.

- Error handling
  - Propagate errors with the `?` operator where appropriate.
  - Prefer returning `Result<..., E>` from fallible library functions rather than panicking.
  - In `main()` or small binary entry points, convert errors to log messages and exit non-zero (the repo uses `eprintln!` and `std::process::exit(1)`; keep that pattern).
  - For web handlers, prefer returning `Result<impl Responder, actix_web::Error>` when handler fallibility is expected; otherwise return `impl Responder` for infallible handlers.
  - Avoid `unwrap()` and `expect()` in library code; if used in tests, ensure the message clarifies the reason.

- Logging
  - The repo currently uses `println!`/`eprintln!` via `utils::log_request`. For production-minded patches prefer adding a structured logging crate (`tracing` or `log`) and keeping logs structured and levels-aware.

- Dependencies
  - Keep dependencies minimal. If adding a crate, prefer well-maintained, widely-used crates.
  - Add dev-dependencies for test-only crates.

- Assets and large files
  - This repo bundles `assets/openhardwaremonitor_localhost_8085_data.json` with `include_str!`. Do not accidentally add large assets to the git history.
  - If an asset becomes large (>1 MB), consider loading it externally or switching to a runtime file read and add it to `.gitignore` if appropriate.

--

## Testing Conventions

- Unit tests: place tests in `#[cfg(test)] mod tests { ... }` inside the corresponding module file for quick access to private members.
- Integration tests: put cross-module tests under `tests/` directory (create when necessary).
- Use `actix_web::test` helpers for HTTP server testing. Initialize an app with `test::init_service(App::new().configure(init_routes)).await` as shown in `src/handlers/mod.rs`.

--

## Agent Operational Rules (must follow)

1. Run `cargo build`, `cargo fmt --all` and `cargo clippy` locally before creating diffs. Fix lint and formatting issues in the same patch.
2. Run tests locally and include failing test output in the PR description if you open one. When making changes that affect tests, run the specific tests you expect to change using the single-test command above.
3. Do not modify files unrelated to the task. Refrain from reverting other people's unstaged changes in the working directory.
4. Do not create commits or push to remotes unless the user explicitly asks you to; if asked, follow repository commit message style and do not force-push.
5. Explain any non-obvious decision in a short comment inside code changes (one-line comments only) and in the PR description.

--

## Cursor / Copilot Rules

- Repository has no `.cursor/rules/` or `.cursorrules` directory at the time of this file creation.
- Repository has no `.github/copilot-instructions.md` file. If such files are added later, follow those rules and include a short summary in this AGENTS.md.

--

## Suggested CI Steps (for maintainers)

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --workspace` (or `cargo test` for this small project)

Add these steps to CI so agent contributions are validated automatically.

--

## Helpful file references

- Application entry: `src/main.rs`
- Server bootstrap: `src/server.rs`
- Routes: `src/routes.rs`
- Handlers: `src/handlers/mod.rs`
- Config: `src/config.rs`
- Utilities: `src/utils/mod.rs`
- Live dashboard: `GET /live` (HTML view over live sensors, health, and schema compatibility)
- Live schema check: `GET /compare/schema` (compares live `/data.json` compatibility against `ohm.json`)
 - Bundled asset: `assets/openhardwaremonitor_localhost_8085_data.json` (an export of LibreHardwareMonitor/OpenHardwareMonitor's `data.json` used to mimic the OHM webserver)

--

If you want, I can also:
1) add a small `rustfmt.toml` with explicit rules,
2) add a `Makefile` or `justfile` with the commands above, or
3) create a starter GitHub Actions workflow for the CI steps described.

Mention which option (1, 2 or 3) you'd like and I'll implement it.
