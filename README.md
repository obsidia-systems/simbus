# 🏭 simbus

## Industrial Field Device Simulator

Simulate realistic Modbus TCP field devices for SCADA labs, integration testing,
and operator training — **no hardware required**.

[![Rust 1.85+](https://img.shields.io/badge/rust-1.85+-DEA584?style=flat-square&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License: MIT](https://img.shields.io/badge/license-MIT-yellow?style=flat-square)](LICENSE)
[![Docker](https://img.shields.io/badge/docker-ready-2496ED?style=flat-square&logo=docker&logoColor=white)](#docker)
[![Modbus TCP](https://img.shields.io/badge/protocol-Modbus%20TCP-FF6B35?style=flat-square)](#architecture)
[![tokio-modbus](https://img.shields.io/badge/tokio--modbus-0.17-blueviolet?style=flat-square)](https://github.com/slowtec/tokio-modbus)
[![axum](https://img.shields.io/badge/axum-0.8-009688?style=flat-square)](https://github.com/tokio-rs/axum)

> **Each container = one device.** Field plane (Modbus TCP, optional TLS on 802, optional OPC UA on 4840) + simulation engine + REST control API.
> Stack as many as you need. Works with Ignition, Wonderware, FactoryTalk, and any Modbus client.

```mermaid
flowchart LR
    subgraph fleet [simbus fleet]
        device[One process: Modbus plus HTTP]
    end
    scada[SCADA Modbus TCP client]
    gui[GUI or tests HTTP client]
    scada -->|FC1 / FC3| device
    gui -->|REST / SSE| device
```

---

## Why simbus?

Building a SCADA lab without physical hardware is painful. Existing Modbus simulators are either
static, hard to script, or impossible to containerize. **simbus** was built to fix that.

| Without simbus | With simbus |
| --- | --- |
| Buy a UPS, PDU, and sensors just to test a tag config | `docker compose up tnh-sensor ups pdu` |
| Static registers that never change | Gaussian noise, drift, sinusoidal cycles, sawtooth |
| Can't test alarm pipelines without breaking real hardware | Inject spikes, freezes, dropouts via REST — on demand |
| Rebuilding state after every test run | `POST /simulation/reset` rewinds everything instantly |
| Hardcoded tag addresses in every test | `GET /config` returns the full register map dynamically |
| Share lab config with the team via Word docs | One YAML file per device — version controlled, reviewable |

---

## Table of Contents

- [Quick Start](#quick-start)
- [Built-in Devices](#built-in-devices)
- [Community devices](#community-devices)
- [Architecture](#architecture)
- [Simulation Behaviors](#simulation-behaviors)
- [Scenarios](#scenarios)
- [REST API Reference](#rest-api-reference)
- [Fault Injection](#fault-injection)
- [Connecting to Ignition](#connecting-to-ignition)
- [Device YAML Schema](#device-yaml-schema)
- [Validating a device YAML](#validating-a-device-yaml)
- [Configuration](#configuration)
- [Logging](#logging)
- [Docker](#docker)
- [Development](#development)
- [Documentation](#documentation)
- [Roadmap](#roadmap)
- [License](#license)

---

## Quick Start

### With Docker (recommended)

```bash
# Start a single T&H sensor (naming a service ignores its profile)
docker compose up tnh-sensor

# Full compose lab: 7 builtin templates + 4 Papouch TH2E instances
docker compose --profile all up
```

```bash
# Verify it's alive
curl http://localhost:8000/status
```

```json
{
  "name": "Generic T&H Sensor",
  "type": "tnh_sensor",
  "modbus_port": 502,
  "modbus_tls_port": null,
  "opcua_port": null,
  "tick_interval": 1.0,
  "time_scale": 1.0,
  "simulation": "running",
  "modbus_server": "listening"
}
```

### From source

```bash
git clone https://github.com/obsidia-systems/simbus.git
cd simbus
git checkout develop
cargo run -p simbus -- --file devices/builtin/generic-tnh-sensor.yaml --port 502 --api-port 8000
```

Native binaries (linux amd64/arm64, macOS Apple Silicon) and a shell installer
ship on [GitHub Releases](https://github.com/obsidia-systems/simbus/releases)
when a `v*` tag lands on `main`:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/obsidia-systems/simbus/releases/latest/download/simbus-installer.sh | sh
```

Until the first tag of this tree, use `cargo build -p simbus`.

### Modbus TLS (IANA 802)

Builtin maps stay **cleartext TCP**. To serve the same PDU over TLS, add a
`modbus-tls` binding (and keep `modbus-tcp` if you want both 502 and 802).
The process does not generate certificates:

```bash
openssl req -x509 -newkey rsa:2048 -nodes -days 365 \
  -keyout key.pem -out cert.pem -subj "/CN=localhost"
```

YAML (do not commit this into `devices/builtin/`):

```yaml
bindings:
  - protocol: modbus-tcp
    port: 502
  - protocol: modbus-tls
    port: 802
    certfile: cert.pem
    keyfile: key.pem
    # cafile: ca.pem   # optional; when set, clients must present a cert
```

```bash
simbus --file device.yaml --modbus-cert cert.pem --modbus-key key.pem
# SCADA: 502 without TLS vs 802 with TLS (trust cert.pem if self-signed)
```

Compose does not mount TLS by default. Add a volume and the binding when you
want the lab.

### OPC UA (IANA 4840)

Builtin maps stay **Modbus TCP only**. To expose the same YAML bank as OPC UA
variables, add an `opcua` binding (and keep `modbus-tcp` if you want both 502
and 4840). This version serves SecurityPolicy None + Anonymous only:

```yaml
bindings:
  - protocol: modbus-tcp
  - protocol: opcua
    port: 4840
```

```bash
simbus --file device.yaml
# UA Expert: opc.tcp://127.0.0.1:4840  (accept None / Anonymous)
```

NodeIds are `ns=N;s=holding/{name}` (and `input/`, `coils/`, `discrete/`).
Values are engineering units (T&H `holding/temperature` ≈ 22.5). Compose does
not publish 4840 by default; add `4840:4840` when you want the lab.

> [!NOTE]
> **Requirements:** Rust 1.85+ (MSRV; edition 2024). Toolchain file tracks
> `stable` — Rust has no official LTS. Docker optional.

---

## Built-in Devices

simbus is a generic measurement engine. Official maps under `devices/builtin/`
are **templates**: point `--file` at one of them. With no file, simbus boots
`devices/builtin/default.yaml` (an example device, not a product). Vendor or
site-specific maps belong in `devices/community/` — see
[Community devices](#community-devices).

```bash
simbus
simbus --file devices/builtin/generic-ups.yaml
simbus --file devices/community/papouch-th2e.yaml
```

Seven product-shaped templates ship ready to use, plus `devices/builtin/default.yaml`
(the zero-arg example device, not a product). Each product template has a realistic
register map, trigger-based alarms, and physics-appropriate simulation.

Host ports below are the `docker-compose.yml` mappings. Inside the container every
generic template listens on Modbus `502` and HTTP `8000`. Compose is not required
for local `cargo run`.

| Device | YAML | Example host map | Holding | Coils |
| --- | --- | --- | --- | --- |
| 🌡️ T&H Sensor | `devices/builtin/generic-tnh-sensor.yaml` | `5020:502`, `8000:8000` | 2 | 2 |
| 🔋 UPS | `devices/builtin/generic-ups.yaml` | `5021:502`, `8001:8000` | 6 | 4 |
| ⚡ PDU | `devices/builtin/generic-pdu.yaml` | `5022:502`, `8002:8000` | 6 | 3 |
| ❄️ CRAC Unit | `devices/builtin/generic-crac.yaml` | `5023:502`, `8003:8000` | 6 | 4 + 1 discrete |
| 📊 Power Meter | `devices/builtin/generic-power-meter.yaml` | `5024:502`, `8004:8000` | 12 | 3 |
| 💧 Leak Sensor | `devices/builtin/generic-leak-sensor.yaml` | `5025:502`, `8005:8000` | 4 | 3 + 1 discrete |
| 🚪 Door Contact | `devices/builtin/generic-door-contact.yaml` | `5026:502`, `8006:8000` | 3 | 4 + 2 discrete |

`default.yaml` is not a Compose service. Papouch TH2E is community (`512` inside the
container; host `5512–5515` / `8100–8103` with `--profile custom` or `--profile all`).

Inspect any running device's full register map (`GET /config`). Excerpt:

```bash
curl http://localhost:8000/config
```

```json
{
  "name": "Generic T&H Sensor",
  "registers": {
    "holding": [
      {"address": 0, "name": "temperature", "unit": "°C", "scale": 10,
       "default": 22.5, "behavior": "gaussian_noise"},
      {"address": 1, "name": "humidity",    "unit": "%RH", "scale": 10,
       "default": 45.0, "behavior": "sinusoidal"}
    ],
    "coils": [
      {"address": 0, "name": "high_temp_alarm",   "default": false},
      {"address": 1, "name": "low_humidity_alarm", "default": false}
    ]
  }
}
```

---

## Community devices

Named products and lab-specific maps live in [`devices/community/`](devices/community/).
Contribute them with a pull request. There is no Rust test per YAML file — validate
locally with `simbus check`, and CI runs the same command on every file under `devices/`.

```bash
cargo run -p simbus -- check devices/community/papouch-th2e.yaml
```

Then run it like any other map:

```bash
cargo run -p simbus -- --file ./devices/community/papouch-th2e.yaml --port 512 --api-port 8000
```

---

## Architecture

Normative picture: [docs/architecture.md](docs/architecture.md). Index:
[docs/README.md](docs/README.md).

```mermaid
flowchart TB
    subgraph container [One container, one device]
        engine[engine tick loop]
        store[(RegisterBank)]
        modbusNode[Modbus TCP]
        api[HTTP control]
        scenario[ScenarioRunner]
        engine -->|writes every tick| store
        store --> modbusNode
        api --> store
        api --> engine
        api --> scenario
        scenario --> engine
        scenario --> store
    end
    scada[SCADA]
    gui[GUI / tests]
    scada -->|FC1 to FC16| modbusNode
    gui -->|REST and SSE| api
```

Tick formulas live in [docs/simulation.md](docs/simulation.md). The tick loop,
Modbus slave, and HTTP control plane share one `RegisterBank`. Scenarios are
bundled in the device YAML.

> [!NOTE]
> ScenarioRunner stays idle until `POST /scenarios/{id}/run`. It does not run
> at boot.

---

## Simulation Behaviors

Every register declares an independent behavior. All behaviors use `state.base` as their operating
point, which means a `PATCH /registers/{address}` call shifts the center and the simulation
**adapts immediately** without a restart.

```mermaid
flowchart LR
    PATCH["PATCH /registers/0\nvalue: 270"] --> BASE["state.base = 27.0°C"]

    BASE --> GN["gaussian_noise\nbase ± std_dev"]
    BASE --> SIN["sinusoidal\nbase + amplitude·sin(t)"]
    BASE --> DR["drift\nbase ± rate × dt"]
    BASE --> SAW["sawtooth\nramps min→max"]
    BASE --> STEP["step\njumps at elapsed_s"]
    BASE --> CONST["constant\nreturns base"]
```

| Behavior | Description | Key parameters |
| --- | --- | --- |
| `constant` | Fixed value | — |
| `gaussian_noise` | Random noise around center | `std_dev` |
| `sinusoidal` | Sine wave oscillation | `period_hours`, `amplitude` |
| `drift` | Slow linear movement with bounds | `rate`, `bounds` |
| `sawtooth` | Ramps from min to max, then resets | `period_seconds`, `min`, `max` |
| `step` | Jumps to defined values at specific elapsed times | `steps: [{at, value}]` |

`gaussian_noise` and `sinusoidal` also support a `drift` sub-modifier that slowly shifts their
center over time.

**Example — temperature with noise and a slow upward drift:**

```yaml
simulation:
  behavior: gaussian_noise
  std_dev: 0.3
  drift:
    enabled: true
    rate: 0.01        # +0.01°C per simulation second
    bounds: [18.0, 35.0]
```

> [!NOTE]
> `--tick` is the wall sample period. `--time-scale` (default `1`) is simulation
> seconds per wall second. See [docs/runtime.md](docs/runtime.md) and
> [docs/simulation.md](docs/simulation.md).

---

## Scenarios

A **scenario** is a timed sequence **bundled in the device YAML**. The process
loads it at boot and leaves it idle until you start it.

Full syntax: [docs/spec.md](docs/spec.md) §7. How to run it:
[docs/control.md](docs/control.md). Operator notes: [docs/scenarios.md](docs/scenarios.md).

Generic T&H ships `heat-wave`, `thermal-runaway`, `fast-alarm-test`, `stuck-sensor`.
Generic UPS ships `power-outage`. The default template ships `demo-spike`.

```bash
curl http://localhost:8000/scenarios
curl -X POST http://localhost:8000/scenarios/heat-wave/run
curl http://localhost:8000/scenarios/active
curl -X POST http://localhost:8000/scenarios/stop
```

---

## REST API Reference

Normative route list: [docs/control.md](docs/control.md). Interactive docs at
**`http://localhost:8000/docs`** (Swagger UI).

### Status and Discovery

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` | `/status` | Name, type, listen Modbus port, tick, `time_scale`, `running`/`stopped`, Modbus `listening`/`stopped` |
| `GET` | `/config` | Contract snapshot — map, `spec_version`, bundled scenarios |
| `GET` | `/healthz` | Liveness (200 if the HTTP task is up) |
| `GET` | `/readyz` | 200 when Modbus is listening and the simulation is running; else 503 |
| `GET` | `/metrics` | Prometheus text |
| `GET` | `/docs` | Swagger UI (`/api-docs/openapi.json` for the spec) |

### Registers

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` | `/registers` | Snapshot of all current raw register values |
| `PATCH` | `/registers/{address}` | Holding — shift operating point, simulation continues from new value |
| `PATCH` | `/registers/input/{address}` | Input — same as above (read-only for Modbus clients, writable via API) |
| `PATCH` | `/registers/coils/{address}` | Coil — set boolean state |
| `PATCH` | `/registers/discrete/{address}` | Discrete input — set boolean state |
| `GET` | `/registers/stream` | **SSE** — current snapshot on subscribe, then each tick and each session write |

Numeric PATCH endpoints accept either a **raw** integer or a **real-world** float — the API applies the register's scale automatically:

```bash
# Real-world value — most ergonomic
curl -X PATCH http://localhost:8000/registers/0 \
  -d '{"real_value": 27.0}'
# → {"address": 0, "raw_value": 270, "real_value": 27.0}

# Raw uint16 — matches Modbus wire format (scale must be known)
curl -X PATCH http://localhost:8000/registers/0 \
  -d '{"value": 270}'
# → {"address": 0, "raw_value": 270, "real_value": 27.0}
```

**Subscribe to the live stream:**

```bash
curl -N http://localhost:8000/registers/stream
# data: {"holding": {"0": 271, "1": 463}, "coils": {"0": false, "1": false}, ...}
# data: {"holding": {"0": 268, "1": 467}, ...}
```

### Simulation Control

| Method | Endpoint | Description |
| --- | --- | --- |
| `PATCH` | `/simulation` | `tick_interval` and/or `running` (pause/resume) |
| `POST` | `/simulation/reset` | Reset all registers to YAML defaults, clear all faults |
| `GET` | `/faults` | Active faults (TTL remaining is simulation seconds) |
| `POST` | `/faults` | Inject a fault |
| `DELETE` | `/faults` | Clear all faults |

### Scenarios

| Method | Endpoint | Description |
| --- | --- | --- |
| `GET` | `/scenarios` | Bundled + session copies (`source`: `bundled` or `session`) |
| `POST` | `/scenarios` | Install a session copy (JSON, same schema as the YAML) |
| `DELETE` | `/scenarios/{name}` | Drop a session copy (not a bundled id) |
| `POST` | `/scenarios/{name}/run` | Start replay (`{name}` is the scenario `id`) |
| `GET` | `/scenarios/active` | Active scenario status (step, elapsed, total) |
| `POST` | `/scenarios/stop` | Cancel any running scenario |

---

## Fault Injection

Faults are **temporary overrides** that expire automatically. Inject them to test alarm pipelines,
edge cases, and failure scenarios without touching real hardware.

```mermaid
sequenceDiagram
    autonumber
    actor Test as Test / CI
    participant API as HTTP control
    participant Engine as engine
    participant SCADA as SCADA
    Test->>API: POST /faults spike
    API->>Engine: inject_fault
    Note over Engine: next tick forces the register
    Engine-->>SCADA: FC3 returns spiked raw
    Note over Engine: TTL expires
    Engine-->>SCADA: normal simulation resumes
```

| Fault type | What happens |
| --- | --- |
| `spike` | Forces a holding or input register to `value` (real units) for the duration |
| `freeze` | Latches the cell’s real value at inject — stuck sensor |
| `dropout` | Forces `0`. Named register, or every holding and input cell when `register_name` is omitted |
| `noise_amplify` | Extra noise with `std_dev × value` (default factor `10`) |
| `alarm` | Forces the **coil** of that name to `true` (skips the trigger). Does not change registers |

```bash
# Spike temperature to trigger high-temp alarm for 60 seconds
curl -X POST http://localhost:8000/faults \
  -H "Content-Type: application/json" \
  -d '{
    "fault_type": "spike",
    "register_name": "temperature",
    "value": 35.0,
    "duration_s": 60
  }'

# Check what's active
curl http://localhost:8000/faults
# [{"fault_type":"spike","register_name":"temperature","value":35.0,
#   "duration_s":60.0,"remaining_s":42.7}]

# Clear everything immediately
curl -X DELETE http://localhost:8000/faults
```

---

## Connecting to Ignition

In **Gateway → Config → OPC-UA → Device Connections → Add → Modbus TCP**:

| Field | Value |
| --- | --- |
| Hostname | `localhost` (or container service name if both are in Docker) |
| Port | Host port (`5020` for tnh-sensor, `5021` for ups…). Device port is typically `502` internally |
| Unit ID | `1` |

**Ignition uses 1-based addressing:**

| YAML address | Ignition tag | Register type |
| --- | --- | --- |
| Holding `0` | `HR1` | Read / Write |
| Holding `N` | `HR{N+1}` | Read / Write |
| Coil `0` | `C1` | Read / Write |
| Input reg `0` | `IR1` | Read only |
| Discrete `0` | `D1` | Read only |

**Apply the scale factor in an Expression Tag:**

```text
{[simbus-tnh-sensor]HR1} / 10.0   →  temperature in °C
{[simbus-tnh-sensor]HR2} / 10.0   →  humidity in %RH
```

**End-to-end alarm test from Ignition:**

```bash
# 1. Inject a spike — Ignition tag HR1 jumps to 35.0°C
curl -X POST http://localhost:8000/faults \
  -d '{"fault_type":"spike","register_name":"temperature","value":35.0,"duration_s":60}'

# 2. Watch the alarm fire in Ignition Alarm Journal

# 3. After 60 seconds fault expires, alarm auto-clears
```

OPC UA (when the YAML lists `protocol: opcua`): in **UA Expert** connect to
`opc.tcp://127.0.0.1:4840`, accept SecurityPolicy **None** and **Anonymous**.
Browse `Objects → Holding` (engineering values, e.g. temperature ≈ 22.5).

---

## Device YAML Schema

The device YAML is the **boot contract**. Language version, fields, validation,
bindings, and bundled scenarios are defined in **[docs/spec.md](docs/spec.md)**.
Session mutations (PATCH, faults, run scenario) are **[docs/control.md](docs/control.md)**.

Minimal example (see spec.md for the full language):

```yaml
name: "My Custom Sensor"
spec_version: 1
version: "1.0"
type: custom_sensor

modbus:
  default_port: 5030
  unit_id: 1
  endianness: big         # big | little | big_swap | little_swap

registers:
  holding:
    - address: 0
      name: pressure
      description: "Line pressure"
      unit: "PSI"
      default: 100.0
      scale: 10            # raw = real_value × scale  →  100 PSI stored as 1000
      data_type: uint16    # uint16 | int16 | uint32 | float32
      simulation:
        behavior: gaussian_noise
        std_dev: 0.5
        drift:
          enabled: true
          rate: 0.02
          bounds: [50.0, 150.0]

  coils:
    - address: 0
      name: overpressure_alarm
      default: false
      trigger:
        source_register: pressure
        condition: gt       # gt | lt | eq | gte | lte
        threshold: 130.0

alarms:
  - name: "Overpressure"
    severity: critical      # info | warning | critical
    trigger: overpressure_alarm
```

> Cross-references are validated at load time — if a coil trigger points to a non-existent
> register, or an alarm references an unknown coil, simbus refuses to start with a clear error.

```yaml
name: "My Custom Sensor"
version: "1.0"
type: custom_sensor

modbus:
  default_port: 5030
  unit_id: 1
  endianness: big         # big | little | big_swap | little_swap

registers:
  holding:
    - address: 0
      name: pressure
      description: "Line pressure"
      unit: "PSI"
      default: 100.0
      scale: 10            # raw = real_value × scale  →  100 PSI stored as 1000
      data_type: uint16    # uint16 | int16 | uint32 | float32
      simulation:
        behavior: gaussian_noise
        std_dev: 0.5
        drift:
          enabled: true
          rate: 0.02
          bounds: [50.0, 150.0]

  coils:
    - address: 0
      name: overpressure_alarm
      default: false
      trigger:
        source_register: pressure
        condition: gt       # gt | lt | eq | gte | lte
        threshold: 130.0

alarms:
  - name: "Overpressure"
    severity: critical      # info | warning | critical
    trigger: overpressure_alarm
```

> Cross-references are validated at load time — if a coil trigger points to a non-existent
> register, or an alarm references an unknown coil, simbus refuses to start with a clear error.

---

## Validating a device YAML

`simbus check` loads the file through the same parser the runtime uses at boot, then
prints a summary of what would be configured. It does not start Modbus or the API.

```bash
cargo run -p simbus -- check devices/community/papouch-th2e.yaml
```

```text
OK  devices/community/papouch-th2e.yaml

  name         Papouch TH2E
  type         papouch_th2e
  ...
  counts       holding 0  input …  coils …  discrete …
```

Exit `0` means the map is valid. Exit `1` prints `FAIL` and the reason. Use this
before opening a community PR — there is no per-file Rust test.

---

## Configuration

All settings use the `SIMBUS_` prefix and can be set via environment variables or CLI flags.

| Variable | Default | Description |
| --- | --- | --- |
| `SIMBUS_YAML_PATH` | default template | Path to a device YAML (`--file`). Omitted → `devices/builtin/default.yaml` (cwd, else embedded) |
| `SIMBUS_MODBUS_PORT` | device YAML default | Override Modbus TCP listen port |
| `SIMBUS_MODBUS_TLS_PORT` | YAML (`802` if omitted) | Override Modbus TLS listen port. Ignored unless the document has `modbus-tls` |
| `SIMBUS_OPCUA_PORT` | YAML (`4840` if omitted) | Override OPC UA listen port. Ignored unless the document has `opcua` |
| `SIMBUS_API_HOST` | `0.0.0.0` | REST API bind address |
| `SIMBUS_API_PORT` | `8000` | REST API listen port |
| `SIMBUS_TICK_INTERVAL` | `1.0` | Wall sample period in seconds (`--tick`) |
| `SIMBUS_TIME_SCALE` | `1.0` | Simulation seconds per wall second (`--time-scale`) |
| `SIMBUS_TICK_HEALTH_LOG_INTERVAL` | `0` | Seconds between `simulation tick health` logs (`0` = off) |
| `SIMBUS_SHUTDOWN_TIMEOUT` | `5.0` | Drain wait after SIGINT/SIGTERM (`0` = abort immediately) |
| `SIMBUS_SEED` | — | RNG seed for reproducible output |
| `SIMBUS_DEVICE_NAME` | — | Override the device name from YAML |
| `SIMBUS_API_KEY` | — | If set, write endpoints require `x-api-key` or `Bearer` |
| `SIMBUS_CORS_ORIGINS` | `*` | Comma-separated CORS origins (`*` for development) |
| `SIMBUS_CTL_URL` | `http://127.0.0.1:8000` | Base URL for `simbus ctl` |

**`.env` example:**

```env
SIMBUS_YAML_PATH=devices/builtin/generic-ups.yaml
SIMBUS_API_PORT=8001
SIMBUS_TICK_INTERVAL=1.0
SIMBUS_CORS_ORIGINS=http://localhost:5173
RUST_LOG=info
```

---

## Logging

simbus uses [`tracing`](https://docs.rs/tracing) with `RUST_LOG` / `tracing-subscriber` env filters.
Typical events include:

- `loading device yaml` / `loading default template` / `loading embedded default template`
- `simbus started` / `simbus stopping`
- `api listening` / `modbus server listening` / `opcua listening`
- `fault injected` / `fault expired` / `faults cleared` / `simulation reset`
- `simulation paused` / `simulation resumed`
- `simulation base changed` / `alarm activated` / `alarm cleared` / `discrete changed`
- `simulation tick health` when `SIMBUS_TICK_HEALTH_LOG_INTERVAL` is > 0 (`tick_interval`, `time_scale`, `tick_duration_ms`, `loop_drift_ms`, `sse_subscribers`, `active_faults`, `uptime_s`)

```bash
RUST_LOG=info cargo run -p simbus -- --file devices/builtin/generic-tnh-sensor.yaml
RUST_LOG=engine=debug,modbus=info cargo run -p simbus -- --file devices/builtin/generic-ups.yaml
```

---

## Docker

Published images: `ghcr.io/obsidia-systems/simbus` (`:X.Y.Z`, `:X.Y`, `:latest`).
A tag `v*` on `main` builds them. Until the first tag of this tree, build locally
(`docker build -t simbus:latest .`).

### Single device

```bash
docker build -t simbus:latest .

docker run -d \
  --cap-add NET_BIND_SERVICE \
  -e SIMBUS_YAML_PATH=/app/devices/builtin/generic-tnh-sensor.yaml \
  -p 5020:502 -p 8000:8000 \
  --name simbus-tnh \
  simbus:latest
```

### Full lab with docker compose

```bash
docker compose --profile all up          # 7 builtin + 4 Papouch (11 containers)
docker compose --profile power up        # UPS + PDU + Power Meter
docker compose --profile env up          # T&H + Leak + Door Contact
docker compose --profile cooling up      # CRAC
docker compose --profile custom up       # Papouch TH2E × 4
docker compose up tnh-sensor ups crac    # handpick by service name
```

Every Compose service has a profile. `docker compose up` with no service names
and no `--profile` starts **nothing**. Naming a service (`up tnh-sensor`) starts
it even without enabling its profile.

The image is a two-stage build (`rust:1-bookworm` compile + `debian:bookworm-slim` runtime),
runs as a non-root user, and health-checks `GET /healthz`. The entrypoint is the `simbus`
binary so Docker behavior matches local runs.

Generic built-in devices listen on Modbus TCP port `502` inside the container and
on API port `8000`. `docker-compose.yml` maps them to unique host ports (`5020`,
`5021`, `8000`, `8001`, etc.). Custom or real devices use the port declared in
their YAML by default. For example, the Papouch TH2E keeps its real device port
`512` internally and is mapped to a high host port such as `5512`.

---

## Development

### Setup

```bash
git clone https://github.com/obsidia-systems/simbus.git
cd simbus
cargo build -p simbus
```

### Run tests

```bash
cargo test --workspace --locked
cargo test -p spec -p engine -p control -p modbus -p simbus --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

### Run locally

```bash
cargo run -p simbus --                                          # default template
cargo run -p simbus -- --file devices/builtin/generic-ups.yaml --port 502 --api-port 8000
cargo run -p simbus -- --file devices/community/papouch-th2e.yaml --port 512 --api-port 8000
cargo run -p simbus -- check devices/community/papouch-th2e.yaml
cargo run -p simbus -- ctl status
```

OpenAPI UI: [http://localhost:8000/docs](http://localhost:8000/docs)

### Project structure

```text
simbus/
├── Cargo.toml              workspace: spec, engine, control, modbus, runtime
├── Cargo.lock
├── crates/
│   ├── spec/               YAML parse + validation + `simbus check` report
│   ├── engine/             RegisterBank, tick, faults, scenario steps
│   ├── control/            axum REST / SSE / metrics / healthz
│   ├── modbus/             tokio-modbus TCP slave
│   └── runtime/            `simbus` binary — one process, one device
├── devices/
│   ├── builtin/            default.yaml + 7 product-shaped templates
│   └── community/          contributor maps (papouch-th2e.yaml)
├── docs/                   contracts (spec, runtime, modbus, control, …)
├── .github/workflows/      CI + GHCR + dist release
├── dist-workspace.toml     dist (GitHub Release binaries)
├── CONTRIBUTING.md         GitFlow (PR to develop)
├── Dockerfile
├── docker-compose.yml      7 builtin services + 4 Papouch (profiles)
├── rust-toolchain.toml
├── rustfmt.toml
├── deny.toml
├── LICENSE
├── .agents/skills/         Agent Skills (`simbus-device` YAML playbook)
├── AGENTS.md               how to change this repo (coding agents)
├── llms.txt                documentation map for agents
├── index.html              marketing landing (not a contract)
└── README.md
```

There is no `scenarios/` folder and no Python tree. Bundled scenarios live in
each device YAML (`scenarios:`). Crate tests: [docs/architecture.md](docs/architecture.md)
§2. Run them with `cargo test -p spec` (or `engine`, `control`, `modbus`,
`simbus`).

### Documentation

Map and Diátaxis roles: **[docs/README.md](docs/README.md)**.

Agents: **[AGENTS.md](AGENTS.md)** (work on this codebase), **[llms.txt](llms.txt)**
(docs map), **[simbus-device skill](.agents/skills/simbus-device/SKILL.md)**
(write a device YAML). In a clone the skill is already there. In another
project:

```bash
npx skills add obsidia-systems/simbus@simbus-device
```

- [docs/architecture.md](docs/architecture.md) — How the process is shaped (diagrams).
- [docs/spec.md](docs/spec.md) — Device YAML language (boot contract).
- [docs/runtime.md](docs/runtime.md) — Binary, boot, CLI/env, signals, Docker.
- [docs/modbus.md](docs/modbus.md) — Field plane: Modbus TCP (V1.1b3 / V1.0b).
- [docs/opcua.md](docs/opcua.md) — Field plane: OPC UA (IANA 4840), YAML map as variables.
- [docs/control.md](docs/control.md) — Session HTTP (not the field protocol).
- [docs/simulation.md](docs/simulation.md) — Tick, `state.base`, behaviors, faults.
- [docs/scenarios.md](docs/scenarios.md) — How to run bundled scenarios.
- [docs/debt.md](docs/debt.md) — Specified as not this version.

New protocols and new YAML fields start in `docs/spec.md` and `crates/spec`.

### Contributing

Contributions are welcome. Open an issue first to discuss significant changes.

Work lands on **`develop`**. Open a `feature/<slug>` branch from `develop` and
open the pull request **against `develop`**, not `main`. `main` is the last
published tree (PR from `develop`, then tag `vX.Y.Z`). Full branch model, CI
gate, and spec-first notes: **[CONTRIBUTING.md](CONTRIBUTING.md)**. Release
steps for agents: [AGENTS.md](AGENTS.md) § Release.

---

## Roadmap

```mermaid
timeline
    title simbus Roadmap
    v0.1 — Core : Modbus TCP, 7 templates, 6 behaviors, REST plus SSE, faults, Docker
    v0.2 — Scenarios : Playback API, recipes later moved into device YAML
    v0.3 — Rust workspace : this tree — tokio-modbus, axum, file-only boot, pause, session scenarios, Modbus TLS 802, OPC UA 4840, healthz/readyz/metrics
    Specified not served : MQTT Sparkplug, SNMP v2c, BACnet/IP, Modbus RTU
```

Unimplemented protocol **syntax** is already valid YAML (`simbus check` notes it;
boot refuses). Serving those protocols is [docs/spec.md](docs/spec.md) §4.
Deferred process work is [docs/debt.md](docs/debt.md). There is no DNP3 binding
in language version 1.

---

## License

simbus is licensed under the **[MIT License](LICENSE)** — use it freely, modify it, ship it,
contribute back.

---

Built with love for industrial automation engineers, SCADA developers, and BMS integrators.
If simbus saves you from buying a UPS just to test a tag config, consider giving it a star.
