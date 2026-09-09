# Debt

Work deferred after closing spec, runtime, engine, control, modbus, and opcua.
Not a product roadmap (MQTT, SNMP, BACnet live in [spec.md](spec.md) §4
and the README timeline).

Index: [README.md](README.md). Architecture: [architecture.md](architecture.md).

When an item is done: delete it from this file and note it in
`CHANGELOG.md`. Spec-first still applies — if the contract is missing,
write the doc before the code.

Pause and session scenario install are in [control.md](control.md)
(this version).

---

## Protocols

Unimplemented protocol **syntax** is valid (`simbus check` notes it; boot
refuses). Serving those protocols is [spec.md](spec.md) §4, not this list.

The Python 0.2.x tree (`simbus/`, `scenarios/`, `pyproject.toml`) is gone.
Do not restore it.

---

## Value shapes (engine)

`square`, `triangle`, `uniform`, and `cycle` are in
[simulation.md](simulation.md) / [spec.md](spec.md) §6. Still not YAML
kinds: cosine (use `sinusoidal` plus `phase_s`), a PID “realistic” walk
(`drift` + `gaussian_noise` is the lab substitute), and OPC UA `qv`
quality (status codes, not a tick behavior — [opcua.md](opcua.md)).
