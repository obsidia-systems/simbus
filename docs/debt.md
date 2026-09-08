# Debt

Work deferred after closing spec, runtime, engine, control, and modbus.
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
