# Changelog

All notable changes to simbus are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased]

### Added

- Rust rewrite of the 0.2.x Python runtime: workspace crates `spec`,
  `engine`, `control`, `modbus`, and binary package `simbus`
  (`crates/runtime`). MSRV 1.85, edition 2024 (tokio, axum, tokio-modbus).
  The Docker image builds that binary (`cargo build --release -p simbus`).
- `simbus check <file>` — validate a device YAML and print a configuration
  summary without starting Modbus or the API. CI runs it on every file under
  `devices/`.
- `devices/builtin/default.yaml` — official example template used when
  `simbus` is started with no `--file` / `SIMBUS_YAML_PATH` (cwd file, else
  the copy embedded in the binary).
- `devices/community/` for contributor maps (PR). Official templates stay in
  `devices/builtin/`.
- Device YAML is the boot contract: `spec_version`, bundled `scenarios:`,
  and protocol bindings (unimplemented protocols are valid syntax; boot refuses
  them). Normative language: `docs/spec.md`. Session API: `docs/control.md`.
  Process: `docs/runtime.md` (SIGINT/SIGTERM, drain then abort leftover).
- `simbus ctl` — HTTP client of an already-running process (`GET /status`,
  PATCH registers, faults, scenarios). Does not boot a device.
- `--time-scale` / `SIMBUS_TIME_SCALE` — simulation seconds per wall second.
  `dt = tick_interval × time_scale`. Default `1` is 1:1.
- `simulation tick health` logs when `SIMBUS_TICK_HEALTH_LOG_INTERVAL` is > 0.
- `AGENTS.md` (how to change this repo), `llms.txt` (documentation map), and
  the Agent Skill `.agents/skills/simbus-device/` for writing device YAML
  (`npx skills add obsidia-systems/simbus@simbus-device`).
- Pause/resume: `PATCH /simulation` `{"running": false}` (tick and scenario
  `at:` freeze; Modbus still serves the last bank; `/readyz` is 503).
  `simbus ctl pause` / `resume`.
- `POST /scenarios` installs a session copy (JSON, same schema as the YAML).
  `DELETE /scenarios/{id}` drops it. Bundled ids return 409.
  `simbus ctl install` / `uninstall`.
- Tick behaviors `square`, `triangle`, `uniform`, and `cycle` (YAML kinds;
  formulas in `docs/simulation.md`). Same live bank as Modbus and OPC UA.
  `cosine` / PID walk / UA `qv` stay deferred (`docs/debt.md`).
- Device language **2**: canonical `points:` (`kind` analog/binary, ASHRAE
  `class` input/value/output) with explicit protocol `export`. Language 1
  register maps still load and are lifted to points. Session API `GET`/`PATCH
  /points/{id}` and `GET /points/stream` (`simbus ctl points` / `set-point`).
  OPC UA language 2 NodeIds are `ns=N;s={id}` under Input/Value/Output folders.
  A language-2 document with no Modbus binding backs every point with a
  private engine cell, so an OPC UA-only or BACnet-only map still has values.
- BACnet/IP field plane: `protocol: bacnet-ip` serves the language-2 `export`
  rows as Analog/Binary Input, Value, and Output objects on IANA **47808**
  (`crates/bacnet`, `bacnet-server`). Who-Is/I-Am, ReadProperty and
  WriteProperty on `Present_Value`; a write lands in the same bank as a
  Modbus FC6 or an HTTP `PATCH /points/{id}`. `device_instance` and a
  non-empty `export` are required; `--bacnet-port` / `SIMBUS_BACNET_PORT`
  overrides the YAML port when the document binds BACnet. `/status` reports
  `bacnet_port` (`null` without a binding) and `/readyz` waits for the
  listener. Official builtin maps stay Modbus-only.
- OPC UA field plane: `protocol: opcua` serves the same YAML map on IANA
  **4840** (`crates/opcua`, async-opcua, None + Anonymous). Dual-bind with
  Modbus TCP/TLS. Official builtin maps stay Modbus-only. `/status.opcua_port`
  is `null` when the document has no UA binding. `cargo deny` allows MPL-2.0
  (async-opcua) and ignores two unfixed transitive advisories in that stack.
- Modbus Security: `protocol: modbus-tls` serves the same V1.1b3 PDU over TLS
  (rustls) on IANA port **802**. Dual-bind with `modbus-tcp` is allowed.
  `certfile`/`keyfile` required at boot; `cafile` optional (mTLS). Official
  builtin maps stay cleartext TCP. Lab recipe: [README.md](README.md).
- Simplified GitFlow (`CONTRIBUTING.md`): feature PRs to `develop`; `main` is
  the last published tree. A `v*` tag on `main` publishes GHCR and native
  binaries via `dist` (linux amd64/arm64, macOS aarch64, shell installer).

### Changed

- Every map under `devices/` is language 2 (`points:` + Modbus `export`).
  The seven product templates were rewritten against the point sets their
  industries actually publish, and the point ids, units, spaces, and data
  types moved with them: UPS on the RFC 1628 UPS MIB object set (charge,
  runtime, seconds on battery, output source, the `upsAlarm*` flags); power
  meter on the Eastron SDM630 float32 input-register map (same zero-based
  addresses, so an existing SDM630 tag list reads it); PDU on the Raritan
  PX / Server Technology Xerus shape (inlet float32 metering, per-outlet
  current, one writable relay coil per outlet, overcurrent protector);
  CRAC on the Liebert iCOM point list (return/supply air, writable
  temperature / humidity / dew-point setpoints, capacity and fan percent,
  filter and airflow alarms); leak detector on locating-panel semantics
  (per-zone leak with distance in metres, cable-break supervision);
  door contact on supervised access monitoring (position, ajar, forced
  entry, request to exit, tamper, loop fault). T&H gained dew point and a
  sensor-fault flag but kept `temperature` and `humidity` at holding 0–1,
  scale 10. `devices/community/papouch-th2e.yaml` is language 2 with the
  same wire layout as before (input 0/1/4/5/8/9, coils 0–3).
  Reading a language-1 document is unchanged.
- `GET /config` reports the declared field plane in `bindings`: protocol,
  port, `unit_id` / `endianness` (Modbus TCP), `device_instance` (BACnet),
  export row count, and whether this runtime implements the protocol. It is
  the document view — CLI and env port overrides show in `/status`, not here.
- The materialized register bank is ordered by address instead of by point
  id, so `simbus check` and `GET /config.registers` read like a register map.
- Boot is file-only: `--file` / `SIMBUS_YAML_PATH`, else the default template.
  There is no `--type` / `SIMBUS_DEVICE_TYPE` / `--devices-dir`. Official maps
  are templates you point `--file` at. The YAML field `type:` remains identity
  inside the document. The default template is also embedded so `simbus` with
  no args works without a checkout.
- Papouch TH2E lives at `devices/community/papouch-th2e.yaml`.
- Bundled scenarios live in the device YAML. The Rust runtime no longer reads
  `SIMBUS_SCENARIO_DIR` / a global `scenarios/` catalog.
- Engine tick contract (`docs/simulation.md`): drift is `rate × dt` (per
  simulation second, not per tick). `uint16` encode clamps instead of wrapping.
  Trigger `eq` matches within half a raw LSB. Freeze latches the cell at inject.
  `POST /simulation/reset` restores boot `RegState` and the RNG stream (same seeded trace). Seed
  mix includes `identity`. Wall clock and simulation time stay 1:1 unless
  `--time-scale` is set; there is no second clock in the engine.
- Control plane (`docs/control.md`, crate `control` not `api`): session HTTP
  vs Modbus field plane. `GET /registers/stream` follows engine ticks and
  session writes. SSE is not covered by the 30 s request timeout. Faults
  validate register/coil names (404) and spike `value` (422). OpenAPI lists
  the full session route set.
- Modbus field plane (`docs/modbus.md`): V1.1b3 / V1.0b. Reads and writes
  exception 02 if the range is not fully implemented. Each PDU address is a
  u16 (a one-word write into a `float32` pair splices that word). Native TCP
  does not filter on MBAP unit id (`0xFF` / `0` are valid per V1.0b).
  `modbus-tls` wraps that PDU in TLS (IANA 802); `/status.modbus_tls_port`
  is `null` when the document has no TLS binding. `/readyz` waits for every
  requested field listener. OPC UA on IANA 4840 is served when the YAML lists
  `protocol: opcua` (`/status.opcua_port`). Architecture and README diagrams
  show tick, Modbus, OPC UA, and HTTP on one bank (boot sequence, shutdown,
  Docker lab).
- Documentation map (`docs/README.md`) and architecture explanation
  (`docs/architecture.md`) with GitHub-safe Mermaid (flowchart, sequence,
  state). README is the front door; contracts stay in `docs/`. Crate tests
  and the device-map table live in those pages; there are no crate or
  `devices/` README files.
- Tracing events from the Rust process: `simbus started` / `simbus stopping`,
  `api listening`, `modbus server listening`, `modbus tls listening`,
  `fault injected` / `expired` /
  `cleared`, `simulation reset`, `simulation paused` / `resumed`,
  `simulation base changed`, `alarm activated` /
  `cleared`, `discrete changed`. There is no `register changed` event.
  `simulation tick health` is emitted when `SIMBUS_TICK_HEALTH_LOG_INTERVAL` > 0.
- SIGINT/SIGTERM: stop accepting, wait up to `SIMBUS_SHUTDOWN_TIMEOUT` (default
  5 s), then abort leftover tasks (including SSE). Timeout `0` aborts immediately.
- CI: `cargo deny check` in the PR gate; GHCR publish requires the `v*` tag
  commit to be an ancestor of `origin/main`. Native binaries and a shell
  installer ship from `.github/workflows/release.yml` (`dist` 0.32). The
  binary Cargo package is `simbus` (`-p simbus`); the crate directory remains
  `crates/runtime`.

### Removed

- Python 0.2.x tree (`simbus/`, `tests/`, `pyproject.toml`, `uv.lock`).
- Global `scenarios/` catalog. Recipes now live under `scenarios:` in each
  device YAML.
- CI job `python-legacy` (`uv` / ruff / mypy / pytest). CI is Rust-only:
  fmt, clippy, `cargo test --workspace --locked`, `cargo deny check`,
  `simbus check` on `devices/`.

---

## [0.2.0] — 2026-04-28

Python runtime (FastAPI + pymodbus). Superseded by the Rust workspace in
Unreleased.

### Added

- Functional logging for simulation/runtime events:
  - `simbus started`
  - `api listening`
  - `modbus server listening`
  - `register changed`
  - `simulation base changed`
  - `fault injected`
  - `fault expired`
  - `faults cleared`
  - `simulation reset`
  - `alarm activated` / `alarm cleared`
- Periodic `simulation tick health` logs with `tick_interval`, `tick_duration_ms`,
  `loop_drift_ms`, `sse_subscribers`, `active_faults`, and `uptime_s`.
- `SIMBUS_TICK_HEALTH_LOG_INTERVAL` setting to control periodic engine health logging.
- `PATCH /registers/input/{address}` — write to an input register from the REST API.
  Input registers are read-only for Modbus clients (FC4), but the simulation control API
  can override them directly. Updates `state.base` so the simulation continues from the
  new value.
- `PATCH /registers/coils/{address}` — set a coil state from the REST API.
  For coils with a trigger condition the value is re-evaluated on the next engine tick.
- `PATCH /registers/discrete/{address}` — set a discrete input state from the REST API.
  Discrete inputs are read-only for Modbus clients (FC2).
- `RegisterOverrideRequest` now accepts `real_value` (float, physical units) as an
  alternative to `value` (raw uint16). The API applies the register's scale factor
  automatically. Mutually exclusive with `value`. Response now returns both
  `raw_value` and `real_value`.
  ```json
  PATCH /registers/0  {"real_value": 27.0}
  → {"address": 0, "raw_value": 270, "real_value": 27.0}
  ```
- Modbus FC6/FC16 writes from external clients (SCADA, PLC) now call
  `engine.update_base()` via a callback wired through `ModbusServerInstance`.
  A SCADA setpoint write now shifts the simulation operating point — identical
  behavior to `PATCH /registers/{address}`.
- `devices/papouch-th2e.yaml` — real device definition for the Papouch TH2E
  Ethernet thermometer/hygrometer with cold-aisle defaults (18 °C / 45 %RH)
  and ASHRAE TC 9.9 alarm thresholds.
- `docs/simulation.md` — full simulation engine reference: tick loop, all 6 behaviors
  with every parameter documented, drift modifier, alarm triggers, all 5 fault types,
  and 7 practical recipes.
- `devices/` folder added to the Docker image (builder `COPY devices/` +
  runtime `COPY --from=builder /app/devices`). Custom YAML device definitions are
  now available inside the container without a volume mount.
- **Scenario Engine** — `ScenarioRunner` replays timed event sequences from YAML files:
  - `GET /scenarios` — list available scenario files
  - `POST /scenarios/{name}/run` — start replaying a scenario
  - `GET /scenarios/active` — runner status (step, elapsed, total)
  - `POST /scenarios/stop` — cancel active scenario
  - Four step types: `set_register`, `inject_fault`, `set_coil`, `set_tick_interval`
  - Runs as independent asyncio task (non-blocking)
  - Steps auto-sorted by `at` timestamp
- `scenarios/heat-wave.yaml` — built-in example scenario with 7 steps simulating a
  temperature rise event, spike fault, and tick interval change.
- `docs/scenarios.md` — full scenario engine reference: schema, step types, runner
  behavior, and practical recipes.
- `simbus/scenarios/` package with `schema.py`, `loader.py`, `engine.py`.

### Fixed

- **`alarm` fault was a no-op.** The fault was handled identically to `spike` —
  it attempted to force a register value keyed by the coil name, which never matched
  any register. Fixed: `_evaluate_alarms` now checks for an active `alarm` fault
  targeting a coil by name and forces that coil to `True`, bypassing trigger evaluation.
  The holding register value is not affected.
- **Discrete inputs wrote to the wrong store.** `_evaluate_alarms` iterated coils and
  discrete inputs in the same loop and called `set_coil()` for both. Discrete trigger
  results were silently written to the coil store instead of the discrete store. Fixed:
  two separate loops — coils use `set_coil`, discrete inputs use `set_discrete`.
- **Coil/discrete triggers on input registers never fired.** `_evaluate_alarms` always
  called `store.get_holding()` to read the source register value, even when the trigger
  referenced an input register. Input register addresses are not in the holding store,
  so the lookup returned `0` and triggers never activated. Fixed: the evaluation now
  selects `get_input()` or `get_holding()` based on whether the source register is
  in `config.registers.input` or `config.registers.holding`.
- **`update_base()` ignored input registers.** The method only searched
  `config.registers.holding`. Fixed: now searches both holding and input registers,
  so `PATCH /registers/input/{address}` correctly updates `state.base`.

### Changed

- Docker runtime now starts through the `simbus` CLI instead of invoking `uvicorn`
  directly. This keeps container startup behavior and logging aligned with local runs.
- Default Uvicorn access logs and noisy `pymodbus` protocol debug output are suppressed
  so runtime logs stay focused on simulation activity and state transitions.
- License changed from Elastic License 2.0 (ELv2) to **MIT**.
- `ModbusServerInstance.__init__` accepts a new optional `on_holding_write` callback
  (signature `(address: int, raw_value: int) -> None`). Pass `engine.update_base` to
  keep the simulation in sync with Modbus client writes.
- `_HoldingBlock.__init__` accepts a new optional `on_write` callback invoked on every
  `setValues` call (FC6/FC16).

---

## [0.1.0] — 2026-04-12

Initial release.

### Added

- **Modbus TCP server** — pymodbus 3.12.x async server per container. Custom
  `BaseModbusDataBlock` subclasses bridge FC1/FC2/FC3/FC4 to `RegisterStore`.
  Zero-offset alignment handled by `_addr()` helper.
- **`RegisterStore`** — in-memory register bank (holding, input, coils, discrete).
  No `asyncio.Lock` needed: all reads/writes are cooperative-safe within a single
  event loop.
- **`SimulationEngine`** — async tick loop with six register behaviors:
  `constant`, `gaussian_noise`, `sinusoidal`, `drift`, `sawtooth`, `step`.
  `DriftModifier` sub-modifier available for `gaussian_noise` and `sinusoidal`.
- **Fault injection** — five fault types with automatic TTL expiry:
  `spike`, `freeze`, `dropout`, `noise_amplify`, `alarm`.
- **Alarm triggers** — coils and discrete inputs auto-updated each tick via
  `trigger:` conditions (`gt`, `lt`, `eq`, `gte`, `lte`).
- **Input register simulation** — input registers run their own behavior on every
  tick (holding and input processed by the same `_tick_registers` helper). Faults
  do not apply to input registers.
- **`state.base` operating point** — all behaviors use `state.base` as their center.
  `PATCH /registers/{address}` calls `update_base()` so the simulation adapts to new
  setpoints without restarting.
- **`reset()` method** — rewinds all registers to YAML defaults, clears faults, resets
  `state.base` and `elapsed_s`. Engine keeps running.
- **Live `tick_interval`** — `engine.tick_interval` is read on every iteration;
  `PATCH /simulation` updates it without a restart.
- **REST API** (FastAPI):
  - `GET  /status` — simulation state, Modbus health, tick interval
  - `GET  /config` — full register map with names, units, scales, behaviors
  - `GET  /registers` — snapshot of all register values
  - `PATCH /registers/{address}` — override holding register + update `state.base`
  - `GET  /registers/stream` — SSE stream, one JSON frame per tick
  - `POST /faults` — inject a fault
  - `GET  /faults` — list active faults
  - `DELETE /faults` — clear all faults
  - `PATCH /simulation` — update tick interval live
  - `POST /simulation/reset` — reset to YAML defaults
- **CORS middleware** — configurable via `SIMBUS_CORS_ORIGINS`.
- **7 built-in devices**: `generic-tnh-sensor`, `generic-ups`, `generic-pdu`,
  `generic-crac`, `generic-power-meter`, `generic-leak-sensor`, `generic-door-contact`.
- **Custom YAML devices** — `SIMBUS_YAML_PATH` or `--file` flag loads any YAML.
  All cross-references (coil triggers, alarm coil names) validated at load time.
- **pydantic-settings** — `DeviceSettings` with `SIMBUS_` env prefix + `.env` support.
- **CLI** — `simbus start --type <builtin> | --file <path> --port N --api-port N`.
- **Docker** — multi-stage build (`uv` builder + `python:3.14-slim` runtime), non-root
  user, healthcheck polling `GET /status`.
- **docker-compose** — profiles (`all`, `power`, `env`, `cooling`, `custom`),
  YAML anchor for shared defaults.
- **131 tests** across five modules:
  `test_config`, `test_behaviors`, `test_simulation_engine`, `test_modbus_server`,
  `test_api`.
