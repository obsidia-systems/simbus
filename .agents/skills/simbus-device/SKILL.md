---
name: simbus-device
description: >-
  Write or review a simbus device YAML (register map, behaviors, alarms,
  bundled scenarios). Use when the user wants a new template, community map,
  sensor, UPS, PDU, CRAC, vendor clone, or `simbus check` of a device file.
license: MIT
compatibility: >-
  Needs a simbus checkout (or `simbus` on PATH) to run `simbus check`.
  Language contract: docs/spec.md in that tree.
metadata:
  author: obsidia-systems
  homepage: https://github.com/obsidia-systems/simbus
---

# Write a simbus device YAML

The YAML **is** the device. Do not invent CLI `--type`, a global `scenarios/`
folder, or a second schema. In a simbus checkout, read `docs/spec.md` first
(where maps live, required fields, `simbus check`). Paths below are from the
repository root, not from this skill folder.

## Where the file goes

| Intent | Path |
| --- | --- |
| Official product-shaped template | `devices/builtin/` (Obsidia only) |
| Named product, real map, lab-specific | `devices/community/<slug>.yaml` (PR) |
| Local experiment | any path; still run `check` |

One file, one device. `type:` is identity (`tnh_sensor`, `papouch_th2e`), not a
runtime selector.

## Pick a starting map

Copy the closest builtin; do not start from a blank page.

| Need | Start from |
| --- | --- |
| Temperature / humidity / analog env | `devices/builtin/generic-tnh-sensor.yaml` |
| Leak / wet contact | `devices/builtin/generic-leak-sensor.yaml` |
| Door / dry contact | `devices/builtin/generic-door-contact.yaml` |
| UPS / battery | `devices/builtin/generic-ups.yaml` |
| PDU / outlets | `devices/builtin/generic-pdu.yaml` |
| kW / energy / PF | `devices/builtin/generic-power-meter.yaml` |
| CRAC / cooling | `devices/builtin/generic-crac.yaml` |
| Vendor PDF / odd port (e.g. 512) / input-only | `devices/community/papouch-th2e.yaml` |
| Tiny example | `devices/builtin/default.yaml` |

## Required document

`name`, `version`, `type`, `modbus` (`default_port`, `unit_id`, `endianness`).
Set `spec_version: 1`. Holding/input names unique across both spaces. Coil and
discrete names unique across both bit spaces. `scale` ≥ 1. No overlapping words
(`float32`/`uint32` occupy `address` and `address+1`).

Behaviors: `constant`, `gaussian_noise`, `sinusoidal`, `drift`, `sawtooth`,
`step`. Drift `rate` is engineering units **per simulation second**. Triggers
(`gt`/`lt`/`eq`/`gte`/`lte`) must name a register that exists. Alarms must name
a coil or discrete that exists.

Scenarios (optional) go in the same file under `scenarios:` with kebab-case
`id`. Step names must exist on **this** map.

Bindings: omit the list to infer Modbus TCP. `modbus-tls` is served (IANA
802, `certfile`/`keyfile` required at boot, `cafile` optional for mTLS). Do
**not** add it to `devices/builtin/` (Compose has no PEM). Dual-bind with
`modbus-tcp` for 502 clear vs 802 TLS. Do not add `modbus-rtu`,
`snmp-v2c`, `opcua`, `mqtt-sparkplug`, or `bacnet-ip` unless the user asked for
syntax-only; `simbus check` accepts them, **boot refuses**.

## Validate (mandatory)

Prefer the binary if it is on `PATH`; otherwise from a simbus checkout:

```bash
simbus check path/to/device.yaml
# or: cargo run -p simbus -- check path/to/device.yaml
```

Exit `0` (`OK`) is the compiler. Exit `1` (`FAIL`): fix the file, do not add a
Rust test. CI already runs `check` on every YAML under `devices/`.

Boot to try it:

```bash
simbus --file path/to/device.yaml --port 502 --api-port 8000
```

## Gotchas

- Restore neither Python, `scenarios/*.yaml`, nor `--type`.
- `POST /scenarios` on a running process is RAM-only. Put recipes in the YAML
  so `simbus check` sees them.
- Live session state (faults, PATCH) is HTTP (`docs/control.md`), not YAML.
- Do not duplicate a register to “make it writable over Modbus” if it should
  be input: keep FC4 and use the control API.
- Do not invent DNP3 or YAML keys that are not in `docs/spec.md`.
