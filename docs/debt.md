# Debt

Work deferred after closing spec, runtime, engine, control, and modbus.
Not a product roadmap (MQTT, SNMP, BACnet live in [spec.md](spec.md) §4
and the README timeline).

Index: [README.md](README.md). Architecture: [architecture.md](architecture.md).

When an item is done: delete it from this file and note it in
`CHANGELOG.md`. Spec-first still applies — if the contract is missing,
write the doc before the code.

---

## Runtime

| Item | Notes |
| --- | --- |
| Tick health log | No `simulation tick health` / `SIMBUS_TICK_HEALTH_LOG_INTERVAL` in this version. Specify in [runtime.md](runtime.md) first. |
| Time acceleration | Wall wait and `dt` are 1:1. A wall-period vs sim-`dt` split MUST be written into [runtime.md](runtime.md) first. |
| Graceful drain | Shutdown aborts tick / Modbus / HTTP. Specify drain in [runtime.md](runtime.md) before implementing. |
| `simbus` HTTP client subcommand | Session control is HTTP only. Specify in [control.md](control.md) first. |

---

## Control / engine (specified, not in this version)

| Item | Notes |
| --- | --- |
| Pause | `is_running` is `/status` and `/readyz` only. `tick()` ignores it. Specify in [control.md](control.md) first. |
| `POST /scenarios` (upload) | Installing a scenario that was not in the YAML is not in this version. Specify in [control.md](control.md) first. |

---

## Protocols

Unimplemented protocol **syntax** is valid (`simbus check` notes it; boot
refuses). Serving those protocols is [spec.md](spec.md) §4, not this list.

The Python 0.2.x tree (`simbus/`, `scenarios/`, `pyproject.toml`) is gone.
Do not restore it.
