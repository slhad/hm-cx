# hm-cx — OpenHardwareMonitor-compatible sensor server

`hm-cx` is a small Rust web server that exposes live host telemetry in the same tree shape used by OpenHardwareMonitor / LibreHardwareMonitor.

It can:
- serve an OHM-compatible `/data.json`
- sample live sensor values from the local machine using `config.yml`
- compare live output against `ohm.json`
- show a live dashboard and mapping console in the browser
- let you override mappings at runtime via `overload.json`

The repository also includes a bundled reference export at:
- `assets/openhardwaremonitor_localhost_8085_data.json`

---

## What this server exposes

### Main routes

| Method | Route | Purpose |
|---|---|---|
| GET | `/` | Simple route index page listing all available endpoints |
| GET | `/live` | Live HTML dashboard for sensors, health, and schema status |
| GET | `/data.json` | OHM-compatible rendered sensor tree |
| GET | `/rawData.json` | Flat raw sensor map keyed by `SensorId` |
| GET | `/snapshot.json` | Current raw in-memory sampler snapshot |
| GET | `/ohm.json` | Filesystem OHM reference JSON if present |
| GET | `/mapping` | Live mapping console with override editor |
| GET | `/mapping.json` | Machine-readable mapping feed |
| GET | `/compare` | Compare raw live data with raw `ohm.json` sensor entries |
| GET | `/compare/schema` | Compare only shape/metadata/units between `ohm.json` and `/data.json` |
| GET | `/compare/view` | HTML side-by-side path comparison view |
| GET | `/health` | Health endpoint with mapping warnings |
| POST | `/generate-config` | Generate `config.yml` automatically |
| POST | `/mapping/overrides` | Save or update one mapping override in `overload.json` |
| POST | `/mapping/overrides/delete` | Remove one mapping override |

---

## Project structure

```text
hm-cx
├── assets/
│   └── openhardwaremonitor_localhost_8085_data.json
├── src/
│   ├── config.rs
│   ├── handlers/mod.rs
│   ├── lib.rs
│   ├── main.rs
│   ├── mapping.rs
│   ├── routes.rs
│   ├── server.rs
│   └── utils/mod.rs
├── tests/
├── Cargo.toml
└── README.md
```

Important files:
- `src/main.rs` — process entry point
- `src/server.rs` — Actix server bootstrap
- `src/routes.rs` — route registration
- `src/handlers/mod.rs` — HTTP handlers and HTML views
- `src/mapping.rs` — mapping resolution, live sampling, override application

---

## Build and run

### Build

```bash
cargo build
```

### Run on default port `8080`

```bash
cargo run
```

### Run on another port

```bash
PORT=8085 cargo run
```

Then open:
- `http://127.0.0.1:8080/` by default
- or the port you selected

---

## CLI commands

### Generate mappings

The server can generate a first-pass `config.yml` automatically:

### Via CLI

```bash
cargo run -- generate-config
```

### Via HTTP

```bash
curl -X POST http://127.0.0.1:8080/generate-config
```

This writes `config.yml` in the working directory.

### Install as a systemd user service

On Linux systems with systemd, the executable can install itself as a user service that starts automatically when you log in.

Recommended flow:

```bash
cargo build --release
./target/release/hm_cx install
```

What `install` does:
- writes `~/.config/systemd/user/hm-cx.service`
- points `ExecStart` at the current executable path
- stores the current working directory as `WorkingDirectory`
- sets `PORT=8085` in the unit so Home Assistant can use the usual OpenHardwareMonitor port by default
- runs `systemctl --user daemon-reload`
- runs `systemctl --user enable --now hm-cx.service`
- runs `systemctl --user restart hm-cx.service` so reinstalling picks up unit changes such as port updates

Important:
- run `install` from the directory that contains the `config.yml`, `ohm.json`, and `overload.json` files you want the service to use
- the installed service listens on port `8085` by default to match OpenHardwareMonitor expectations
- if you want a different port, override `PORT` in the user unit after installation
- if you install from `cargo run -- install`, the service will point at Cargo's debug build output instead of a stable release binary

### Uninstall the systemd user service

```bash
./target/release/hm_cx uninstall
```

What `uninstall` does:
- runs `systemctl --user disable --now hm-cx.service`
- removes `~/.config/systemd/user/hm-cx.service`
- runs `systemctl --user daemon-reload`

You can inspect the installed unit with:

```bash
systemctl --user status hm-cx.service
systemctl --user cat hm-cx.service
```

### Linux capabilities for package power / RAPL

Some power sensors, especially CPU package power exposed via `powercap_rapl`, are read from files such as:

```text
/sys/class/powercap/.../energy_uj
```

On many Linux systems those files are not readable by an unprivileged process. In that case `hm-cx` needs these capabilities on the server binary:

- `cap_dac_read_search`
- `cap_perfmon`

For example, if `Package` is mapped to `powercap_rapl` (such as `/amdcpu/0/power/0` in an OHM-compatible tree), apply the capabilities to the exact binary you run:

```bash
cargo build --release
./scripts/set-hm-cx-capability.sh target/release/hm_cx
getcap ./target/release/hm_cx
```

Expected `getcap` output is similar to:

```text
./target/release/hm_cx cap_dac_read_search,cap_perfmon=ep
```

Notes:
- capabilities are attached to the binary file, not to `config.yml` or the systemd unit
- rebuilding or replacing the binary usually clears them, so re-run the script after upgrades or fresh builds
- if you use `install`, apply capabilities to the same path used by `ExecStart` and then restart the service

---

## Live sampling behavior

If `config.yml` exists when the server starts, `hm-cx` launches a background sampler.

In live mode:
- `/data.json` becomes authoritative
- the sampler updates the in-memory snapshot continuously
- the HTML views (`/live`, `/mapping`) refresh against live endpoints

If no `config.yml` exists:
- `/data.json` falls back to the bundled reference asset
- `/rawData.json` falls back to raw data extracted from the bundled asset

### `/data.json` responses in live mode

- `200 OK` when at least one sensor value was produced
- `500 Internal Server Error` when the snapshot is empty or `null`

Example error payload:

```json
{
  "error": "no sensor data available",
  "reason": "mappings produced no values",
  "snapshot_size": 0,
  "timestamp_unix": 1670000000,
  "suggestion": "inspect /snapshot.json and verify config.yml mappings"
}
```

---

## Mapping model

### `config.yml`

`config.yml` contains the default mapping from an OHM sensor id to a local data source.

Example:

```yaml
mappings:
  - ohm: /test/temperature/0
    text: Test Temp
    type: Temperature
    source:
      kind: sysfs
      path: /sys/class/hwmon/hwmon0/temp1_input
      chip: null
      key: null
```

Supported source kinds used by the mapper:
- `sysfs`
- `sensors_json`
- `powercap_rapl`
- `proc_cpuinfo`

### `overload.json`

`overload.json` is the manual override layer applied on top of `config.yml`.

Example:

```json
{
  "overrides": [
    {
      "ohm": "/test/temperature/0",
      "source": {
        "kind": "sysfs",
        "path": "/sys/class/hwmon/hwmon9/temp9_input",
        "chip": null,
        "key": null
      }
    }
  ]
}
```

Behavior:
- overrides are reloaded continuously during sampling
- overrides take precedence over generated/default mappings
- overridden sensor nodes in `/data.json` get an extra field:

```json
{
  "Value": "42.00 °C",
  "RawValue": "42000",
  "Text": "CPU",
  "Type": "Temperature",
  "overload": true
}
```

The extra `overload` field is intentionally ignored by `/compare/schema` so schema compatibility checks still focus on OHM shape and metadata.

---

## Mapping console

Open:

```text
/mapping
```

The mapping console shows:
- OHM sensor id
- current tree path inside rendered `/data.json`
- live value and raw value
- default source from `config.yml`
- effective source after override application
- whether the mapping is overloaded

It polls `/mapping.json` every 5 seconds.

From the browser you can:
- save an override to `overload.json`
- remove an existing override

### `GET /mapping.json`

Returns a live summary like:

```json
{
  "configured": true,
  "live_snapshot": true,
  "override_file": "overload.json",
  "override_count": 1,
  "mappings": [
    {
      "ohm": "/test/temperature/0",
      "text": "CPU",
      "type": "Temperature",
      "tree_path": "Children/[0]",
      "label_path": "Sensor / CPU",
      "live_value": "42.00 °C",
      "live_raw_value": "42000",
      "default_source": { "kind": "sysfs", "path": "/sys/class/hwmon/hwmon0/temp1_input", "chip": null, "key": null },
      "effective_source": { "kind": "sysfs", "path": "/sys/class/hwmon/hwmon9/temp9_input", "chip": null, "key": null },
      "override_source": { "kind": "sysfs", "path": "/sys/class/hwmon/hwmon9/temp9_input", "chip": null, "key": null },
      "overloaded": true
    }
  ]
}
```

### Save an override

```bash
curl -X POST http://127.0.0.1:8080/mapping/overrides \
  -H 'Content-Type: application/json' \
  -d '{
    "ohm": "/test/temperature/0",
    "source": {
      "kind": "sysfs",
      "path": "/sys/class/hwmon/hwmon9/temp9_input",
      "chip": null,
      "key": null
    }
  }'
```

### Delete an override

```bash
curl -X POST http://127.0.0.1:8080/mapping/overrides/delete \
  -H 'Content-Type: application/json' \
  -d '{"ohm":"/test/temperature/0"}'
```

---

## Compare and validation routes

### `GET /compare`

Compares:
- raw entries extracted from filesystem `ohm.json`
- current live raw data

Useful for seeing key-level differences between reference and live raw sensor maps.

### `GET /compare/schema`

Checks compatibility of live `/data.json` against `ohm.json`.

It validates:
- path presence
- metadata fields like `SensorId`, `Text`, `Type`
- displayed unit compatibility for fields such as `Value`, `Min`, `Max`

It intentionally ignores:
- live numeric drift
- OHM `-` values that mean “unavailable at capture time”
- the extra `overload` marker added by manual overrides

### `GET /compare/view`

HTML view for side-by-side hierarchical key comparison between:
- `ohm.json`
- current `/data.json`

---

## Health and debug routes

### `GET /health`

Returns:
- `ok` when live snapshot exists and no warnings are detected
- `degraded` when mappings exist but some backing sources are missing/inaccessible
- `unhealthy` when no live data is available in live mode

It also reports warnings for common issues such as:
- missing sysfs paths
- unreadable `powercap_rapl` files

### `GET /snapshot.json`

Returns the raw in-memory snapshot exactly as held by the sampler.

### `GET /rawData.json`

Returns a flat map keyed by `SensorId`.

This is useful when debugging mapping values independently of the OHM tree.

### `GET /ohm.json`

Returns the local `ohm.json` file if present.

---

## FAQ / Troubleshooting

### Package power is missing, blank, or unreadable

Typical symptoms:
- `/health` reports a warning about `powercap_rapl`
- a `Package` power sensor never gets a live value
- logs mention permission denied when reading `/sys/class/powercap/.../energy_uj`

Useful checks:

```bash
# See how power sensors are mapped
rg -n "powercap_rapl|/power/" config.yml overload.json

# Check the health endpoint for permission warnings
curl -s http://127.0.0.1:8080/health | jq .

# Inspect live power mappings and values
curl -s http://127.0.0.1:8080/mapping.json | jq '.mappings[] | select(.type == "Power") | {ohm, text, effective_source, live_value, live_raw_value}'

# See which RAPL files exist on this host
find /sys/class/powercap -maxdepth 3 \( -name energy_uj -o -name name \) -print

# Check whether the hm-cx binary already has the required capabilities
getcap ./target/release/hm_cx

# Apply them if needed
./scripts/set-hm-cx-capability.sh target/release/hm_cx
```

If you run `hm-cx` as a user service, also verify and restart it after changing capabilities:

```bash
systemctl --user status hm-cx.service
systemctl --user cat hm-cx.service
systemctl --user restart hm-cx.service
```

If the binary was rebuilt after capabilities were applied, re-run `setcap` or `scripts/set-hm-cx-capability.sh` on the new binary.

## Logging

The server uses `tracing`.

Examples:

```bash
cargo run
RUST_LOG=debug cargo run
PORT=8085 RUST_LOG=info cargo run
```

Example warning you may see:

```text
mapping produced no value
```

This usually means a configured mapping did not produce a live reading during sampling.

---

## Testing and quality checks

Run locally before submitting changes:

```bash
cargo fmt --all
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Useful targeted test examples:

```bash
cargo test data_json_route_returns_the_bundled_asset -- --exact
cargo test data_json -- --nocapture
```

---

## Current workflow recommendation

1. Generate or create `config.yml`
2. Start the server
3. Open `/mapping` to inspect effective mappings
4. Add overrides where needed
5. Verify `/data.json`
6. Check `/compare/schema` for compatibility
7. Use `/live` for a compact live dashboard

---

## License

MIT
