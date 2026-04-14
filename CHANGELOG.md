# Changelog

All notable changes to simbus are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

---

## [Unreleased]

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
