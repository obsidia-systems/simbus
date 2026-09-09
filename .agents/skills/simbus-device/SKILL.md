---
name: simbus-device
description: >-
  Write or review a simbus device YAML (points, protocol export, behaviors,
  alarms, bundled scenarios). Use when the user wants a new template, community
  map, sensor, UPS, PDU, CRAC, vendor clone, or `simbus check` of a device file.
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

Language **2** is current (`points:` + explicit `export` on each binding).
Language **1** (`registers:` holding/input/coils/discrete) still loads; the
runtime lifts it to points. **New maps SHOULD use `spec_version: 2`.** Official
product templates under `devices/builtin/` except `default.yaml` may still be
language 1 until rewritten. Do not mix `points:` and `registers:` in one file.

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
| Tiny language-2 example | `devices/builtin/default.yaml` |
| Temperature / humidity / analog env | `devices/builtin/generic-tnh-sensor.yaml` |
| Leak / wet contact | `devices/builtin/generic-leak-sensor.yaml` |
| Door / dry contact | `devices/builtin/generic-door-contact.yaml` |
| UPS / battery | `devices/builtin/generic-ups.yaml` |
| PDU / outlets | `devices/builtin/generic-pdu.yaml` |
| kW / energy / PF | `devices/builtin/generic-power-meter.yaml` |
| CRAC / cooling | `devices/builtin/generic-crac.yaml` |
| Vendor PDF / odd port (e.g. 512) / input-only | `devices/community/papouch-th2e.yaml` |

## Language 2 document

`name`, `version`, `type`, `spec_version: 2`, `points`, `bindings` with
**non-empty `export`** for every served protocol. Empty `bindings` does **not**
infer Modbus (HTTP-only). Point `id` unique. `kind` is `analog` or `binary`.
`class` is ASHRAE `input` / `value` / `output` (who owns the live value — not
which Modbus table). Analog `default` is a number; binary `default` is a bool.

Engine values are engineering `f64` and `bool`. Integer `scale` / `data_type`
exist only on **Modbus export**. Modbus `class` does **not** pick the table:
a sensor MAY live in `holding`. Export is always explicit (omitting it does
not publish every point).

```yaml
spec_version: 2
points:
  - id: analog_a
    kind: analog
    class: value
    unit: units
    default: 50.0
    simulation:
      behavior: gaussian_noise
      std_dev: 1.5
  - id: analog_a_high
    kind: binary
    class: input
    default: false
    trigger:
      source: analog_a
      condition: gt
      threshold: 80.0
bindings:
  - protocol: modbus-tcp
    port: 502
    unit_id: 1
    endianness: big
    export:
      analog_a: { space: holding, address: 0, scale: 10, data_type: uint16 }
      analog_a_high: { space: coil, address: 0 }
```

Trigger `source` (alias `source_register`) MUST name a **point id**. Alarms
MUST name a binary point id. Scenario steps: prefer `set_point` /
`inject_fault` with `point:`. `set_register` / `set_coil` still work when the
export materialized those names.

Behaviors (analog only): `constant`, `gaussian_noise`, `sinusoidal`, `square`,
`drift`, `sawtooth`, `triangle`, `uniform`, `step`, `cycle`. `square` is analog
high/low around `state.base`. `triangle` ramps min↔max (unlike `sawtooth`,
which only rises). `uniform` is Uniform(min, max) each tick, not Normal
around `state.base`. `cycle` walks `values` every `dwell_seconds` (unlike
`step`, which is an absolute `at` schedule). Drift `rate` is engineering
units **per simulation second**.

## Language 1 document (still valid)

`name`, `version`, `type`, `modbus` (`default_port`, `unit_id`, `endianness`).
`spec_version` omitted or `1`. Holding/input names unique across both spaces.
Coil and discrete names unique across both bit spaces. `scale` ≥ 1. No
overlapping words (`float32`/`uint32` occupy `address` and `address+1`).
Triggers (`gt`/`lt`/`eq`/`gte`/`lte`) must name a register that exists. Alarms
must name a coil or discrete that exists. Empty `bindings` infers Modbus TCP.

## Bindings

`modbus-tls` is served (IANA 802, `certfile`/`keyfile` required at boot,
`cafile` optional for mTLS). Do **not** add it to `devices/builtin/` (Compose
has no PEM). Dual-bind with `modbus-tcp` for 502 clear vs 802 TLS. `opcua` is
served (IANA 4840, None + Anonymous). Language 2 NodeIds are `ns=N;s={id}`
under Input/Value/Output folders; language 1 stays `holding/{name}`. Do
**not** add `opcua` to `devices/builtin/` (Compose does not publish 4840).
`bacnet-ip` is served (IANA 47808, language 2 only). It needs
`device_instance` and an `export` map of `{point-id: {object, instance}}`
where `object` is `analog-input` / `analog-value` / `analog-output` /
`binary-input` / `binary-value` / `binary-output`. `(object, instance)` pairs
must be unique. Do **not** add `bacnet-ip` to `devices/builtin/` (Compose does
not publish 47808). On a document that also binds Modbus, a BACnet row only
has a value when that id is in the Modbus `export` too — `check` warns
otherwise. Language 2 `opcua` / `bacnet-ip` need a non-empty `export`. Do not
add `modbus-rtu`, `snmp-v2c`, or `mqtt-sparkplug` unless the user asked for
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
  Prefer `PATCH /points/{id}` (`{"value": 27.0}` or `{"value": true}`). Address
  routes `/registers/…` remain for language 1 and materialized exports.
- Do not duplicate a point to “make it writable over Modbus” if it should be
  input: export `space: input` and use the control API.
- Do not invent DNP3 or YAML keys that are not in `docs/spec.md`.
