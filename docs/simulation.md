# Simulation Engine — Reference Guide

This document covers the simulation engine in full: how the tick loop works, every
behavior type and its parameters, the drift modifier, alarm triggers, and fault injection.
Practical recipes are included throughout.

---

## Table of Contents

- [How the Engine Works](#how-the-engine-works)
- [Operating Point — state.base](#operating-point--statebase)
- [Behaviors](#behaviors)
  - [constant](#constant)
  - [gaussian\_noise](#gaussian_noise)
  - [sinusoidal](#sinusoidal)
  - [drift](#drift)
  - [sawtooth](#sawtooth)
  - [step](#step)
- [Drift Modifier](#drift-modifier)
- [Alarm Triggers](#alarm-triggers)
- [Fault Injection](#fault-injection)
  - [spike](#spike)
  - [freeze](#freeze)
  - [dropout](#dropout)
  - [noise\_amplify](#noise_amplify)
  - [alarm](#alarm)
- [Practical Recipes](#practical-recipes)

---

## How the Engine Works

The `SimulationEngine` runs as a single asyncio Task. On every **tick** it:

1. Decrements fault timers and removes expired faults.
2. Iterates every holding and input register with a `simulation` block.
3. Advances the register's elapsed simulation time by `tick_interval` seconds.
4. Calls the behavior function to compute a new real-world value from `state.base`.
5. Applies any active fault that targets this register (holding only — input registers
   are read-only to Modbus clients and are not affected by faults).
6. Scales the real-world value to a raw integer (`raw = real × scale`) and writes it
   to the `RegisterStore`.
7. Evaluates all coil and discrete-input triggers against current register values.
8. Publishes a JSON snapshot to every active SSE subscriber.

```
tick
 ├─ tick_faults(dt)           — expire TTL faults
 ├─ _tick_registers(holding)  — compute + fault + write to holding store
 ├─ _tick_registers(input)    — compute + write to input store (no faults)
 └─ _evaluate_alarms()        — update coils and discrete inputs
```

### Cooperative safety — no locks

asyncio is single-threaded and cooperative. Because neither `_tick()` nor
`RegisterStore.get/set` ever `await`, they execute atomically relative to all other
coroutines (FastAPI handlers, Modbus DataBlock callbacks). No `asyncio.Lock` is needed.

### Tick interval

`tick_interval` (seconds) controls both the real-time pace and the simulation time step.
The engine reads `self.tick_interval` on **every iteration**, so it can be updated live:

```bash
# Slow down to one tick every 5 seconds
curl -X PATCH http://localhost:8000/simulation \
  -H "Content-Type: application/json" \
  -d '{"tick_interval": 5.0}'

# Speed up to 10 ticks per second
curl -X PATCH http://localhost:8000/simulation \
  -d '{"tick_interval": 0.1}'
```

For **time acceleration** — simulating hours of device behavior in seconds — set
`SIMBUS_TICK_INTERVAL=60.0`. Each real second advances the simulation by 60 seconds,
making a 12-hour sinusoidal cycle complete in 12 real minutes.

---

## Operating Point — state.base

Every register has a mutable `state.base` value initialized from `default` in the YAML.
All behaviors use `state.base` as their center or starting point.

When you `PATCH /registers/{address}`, the engine calls `update_base()`, which converts
the raw integer back to a real-world value (`raw / scale`) and stores it as the new
`state.base`. On the next tick, the behavior runs from the new operating point — it
doesn't snap back to the YAML default.

```
YAML default: temperature = 22.5°C  →  state.base = 22.5
gaussian_noise oscillates around 22.5°C

PATCH /registers/0 {"value": 270}   →  raw 270 / scale 10 = 27.0°C
state.base = 27.0
gaussian_noise now oscillates around 27.0°C — takes effect next tick
```

This is the correct way to simulate an operator adjusting a setpoint, changing a
load condition, or positioning a test at a known operating point before injecting a fault.

---

## Behaviors

Behaviors are declared under `simulation:` in the register block. Each behavior is
identified by the `behavior:` key, which selects the Pydantic model and the
computation function.

Registers without a `simulation:` block are static — they hold their default value
(or whatever value was written by a PATCH) and are never updated by the engine.

---

### constant

Returns `state.base` unchanged every tick. The value never drifts or adds noise.
Useful for setpoints, configuration registers, or any value that should only change
when explicitly written.

```yaml
- address: 5
  name: setpoint
  unit: "°C"
  default: 18.0
  scale: 10
  simulation:
    behavior: constant
```

**Parameters:** none.

**Use cases:**
- Setpoint registers that operators write to
- Mode or status registers that change only on command
- Reference values in multi-register devices

---

### gaussian\_noise

Adds normally distributed (Gaussian) noise to `state.base` on every tick. The
output oscillates around the base value — most samples fall within ±1 std\_dev,
~95% within ±2 std\_dev.

```yaml
- address: 0
  name: temperature
  unit: "°C"
  default: 22.5
  scale: 10
  simulation:
    behavior: gaussian_noise
    std_dev: 0.3
```

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `std_dev` | float > 0 | yes | Standard deviation of the noise in real-world units |
| `drift` | object | no | Optional drift modifier (see [Drift Modifier](#drift-modifier)) |

**How to choose `std_dev`:**
- Precision sensor (±0.1°C): `std_dev: 0.05`
- Typical HVAC sensor (±0.5°C): `std_dev: 0.3`
- Noisy current sensor (±2%): `std_dev: 1.0`
- Intentionally unstable / degraded sensor: `std_dev: 5.0`

**Use cases:**
- Temperature, humidity, pressure, current, voltage sensors
- Any value that should appear "live" without a defined pattern

```yaml
# Voltage with very tight noise — stable supply
- address: 1
  name: input_voltage
  unit: "V"
  default: 120.0
  scale: 10
  simulation:
    behavior: gaussian_noise
    std_dev: 0.2

# Fan speed with more noise — mechanical vibration
- address: 3
  name: fan_speed
  unit: "%"
  default: 70.0
  scale: 10
  simulation:
    behavior: gaussian_noise
    std_dev: 1.5
```

---

### sinusoidal

Produces a sine wave oscillation around `state.base`. The value cycles between
`base - amplitude` and `base + amplitude` over the configured period.

```
value = state.base + amplitude × sin(2π × elapsed_s / period_s)
```

```yaml
- address: 1
  name: humidity
  unit: "%RH"
  default: 45.0
  scale: 10
  simulation:
    behavior: sinusoidal
    period_hours: 12
    amplitude: 5.0
```

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `period_hours` | float > 0 | yes | Full cycle duration in hours |
| `amplitude` | float > 0 | yes | Peak deviation from center in real-world units |
| `drift` | object | no | Optional drift modifier (see [Drift Modifier](#drift-modifier)) |

**Examples:**

```yaml
# Daily temperature cycle — ±3°C over 24 hours
simulation:
  behavior: sinusoidal
  period_hours: 24
  amplitude: 3.0

# Fast oscillation for testing — full cycle every 2 minutes (0.033 h)
simulation:
  behavior: sinusoidal
  period_hours: 0.033
  amplitude: 10.0

# UPS load — peaks every 2 hours, ±10% around baseline
simulation:
  behavior: sinusoidal
  period_hours: 2
  amplitude: 10.0
```

**Time acceleration tip:**
At `SIMBUS_TICK_INTERVAL=60.0`, each real second = one simulation minute.
A `period_hours: 24` cycle completes in 24 real minutes instead of 24 hours.

**Use cases:**
- Daily temperature/humidity cycles in data centers or facilities
- Periodic load patterns on UPS and PDUs
- HVAC return air temperature variation
- Any value with a known repeating period

---

### drift

Moves `state.base` by a fixed `rate` each tick, clamped within `bounds`. When the
value reaches a bound it stops (does not bounce back). To simulate discharge/charge
cycles, inject a fault or use `PATCH /registers` to reposition the base.

```yaml
- address: 0
  name: battery_soc
  unit: "%"
  default: 100.0
  scale: 10
  simulation:
    behavior: drift
    rate: -0.005      # negative = decreasing
    bounds: [0.0, 100.0]
```

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `rate` | float | yes | Change per tick in real-world units. Negative = downward |
| `bounds` | [float, float] | yes | `[min, max]` hard clamp. `bounds[0]` must be < `bounds[1]` |

**Rate sizing guide** (at default `tick_interval=1.0`):

| Goal | rate |
| --- | --- |
| 1% drop per minute | `-0.0167` (per second tick) |
| 1% drop per 200 ticks | `-0.005` |
| Temperature rises 1°C per hour | `+0.000278` |
| Fast discharge for testing | `-0.5` |

```yaml
# Runtime counter — drains over ~100 minutes (6000 ticks at 1s)
- address: 4
  name: runtime_remaining
  unit: "min"
  default: 60.0
  scale: 1
  simulation:
    behavior: drift
    rate: -0.01
    bounds: [0.0, 120.0]

# Slow upward drift — aging sensor baseline creep
- address: 2
  name: co2_level
  unit: "ppm"
  default: 400.0
  scale: 1
  simulation:
    behavior: drift
    rate: 0.1
    bounds: [350.0, 5000.0]
```

**Use cases:**
- Battery state of charge / discharge
- Runtime remaining counters
- Slow sensor aging or baseline creep
- Gradual temperature rise in an enclosure

---

### sawtooth

Linearly ramps from `min` to `max` over `period_seconds`, then immediately resets to
`min` and repeats. The shape is a rising ramp, not a triangle — there is no descending
slope.

```
value = min + (max - min) × (elapsed_s % period_s) / period_s
```

```yaml
- address: 3
  name: compressor_cycle
  unit: "%"
  default: 0.0
  scale: 10
  simulation:
    behavior: sawtooth
    period_seconds: 300   # 5-minute cycle
    min: 0.0
    max: 100.0
```

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `period_seconds` | float > 0 | yes | Full cycle duration in seconds |
| `min` | float | yes | Starting (and reset) value |
| `max` | float | yes | Peak value. Must be > `min` |

**Use cases:**
- Compressor or pump cycle simulation
- Load ramp tests — verify alarm response at specific thresholds
- Coolant pressure cycles
- Any repeating linear ramp pattern

**Alarm ramp test example:**
Set `min` below alarm threshold and `max` above it, then watch the coil fire and clear
on each cycle without any manual intervention.

```yaml
# Temperature ramps through alarm threshold every 10 minutes
simulation:
  behavior: sawtooth
  period_seconds: 600
  min: 20.0    # below high_temp_alarm threshold of 30.0°C
  max: 40.0    # above threshold
```

---

### step

Holds a series of discrete values at scheduled simulation times. Stays at `default`
until the first step threshold, then holds each step's value until the next one.
The last step value is held indefinitely.

```yaml
- address: 1
  name: battery_mode
  default: 0.0
  scale: 1
  simulation:
    behavior: step
    steps:
      - at: 0       # seconds from simulation start
        value: 1.0  # online mode
      - at: 300     # 5 minutes in
        value: 2.0  # on-battery mode
      - at: 600     # 10 minutes in
        value: 3.0  # low-battery mode
      - at: 900     # 15 minutes in
        value: 0.0  # shutdown
```

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `steps` | list | yes | At least one entry. Each entry has `at` (seconds) and `value` |
| `steps[].at` | float ≥ 0 | yes | Elapsed simulation seconds when this step becomes active |
| `steps[].value` | float | yes | Real-world value to hold from this point on |

Steps are evaluated in ascending `at` order. If multiple steps share the same `at`,
the last one in the list wins.

**Use cases:**
- Mode registers that change during a test scenario
- Simulating a device startup sequence
- Discrete state machines (standby → active → fault → reset)
- Pre-planned test sequences without needing the fault API

**Combined step + alarm trigger:**

```yaml
registers:
  holding:
    - address: 0
      name: load_kw
      default: 0.0
      scale: 100
      simulation:
        behavior: step
        steps:
          - at: 0      value: 10.0   # normal load
          - at: 60     value: 45.0   # heavy load — stays under alarm threshold
          - at: 120    value: 55.0   # overload — crosses threshold, fires alarm
          - at: 180    value: 10.0   # recovers

  coils:
    - address: 0
      name: overload_alarm
      trigger:
        source_register: load_kw
        condition: gt
        threshold: 50.0
```

---

## Drift Modifier

`gaussian_noise` and `sinusoidal` can include an optional `drift:` block that slowly
shifts their center (`state.base`) over time. This models long-term sensor creep,
gradual environmental changes, or a slowly worsening condition.

```yaml
simulation:
  behavior: gaussian_noise
  std_dev: 0.3
  drift:
    enabled: true
    rate: 0.01        # center drifts +0.01°C per tick
    bounds: [18.0, 35.0]
```

| Parameter | Type | Required | Description |
| --- | --- | --- | --- |
| `enabled` | bool | no (default: `true`) | Set to `false` to define the modifier but disable it |
| `rate` | float | yes | Change in `state.base` per tick. Negative = drift downward |
| `bounds` | [float, float] | yes | Hard clamp on `state.base`. `bounds[0]` must be < `bounds[1]` |

The drift modifier runs **before** the behavior computation, so the noise or sine wave
is always centered on the current (drifted) base — the output gradually migrates.

```yaml
# T&H sensor: temperature slowly drifts toward 35°C, noise oscillates around the moving center
simulation:
  behavior: gaussian_noise
  std_dev: 0.3
  drift:
    enabled: true
    rate: 0.01
    bounds: [18.0, 35.0]

# Sinusoidal humidity with a slow downward drift — gradually drying out
simulation:
  behavior: sinusoidal
  period_hours: 6
  amplitude: 5.0
  drift:
    enabled: true
    rate: -0.002
    bounds: [20.0, 80.0]
```

---

## Alarm Triggers

Coils and discrete inputs can declare a `trigger:` block that automatically sets their
value based on a holding (or input) register crossing a threshold. The engine evaluates
all triggers on every tick after computing new register values.

```yaml
coils:
  - address: 0
    name: high_temp_alarm
    default: false
    trigger:
      source_register: temperature   # name of a holding or input register
      condition: gt                  # gt | lt | eq | gte | lte
      threshold: 30.0                # real-world value (not raw)
```

| Field | Description |
| --- | --- |
| `source_register` | Name of the register to watch. Must exist in `holding` or `input`. Validated at load time. |
| `condition` | Comparison operator: `gt` (>), `lt` (<), `eq` (==), `gte` (>=), `lte` (<=) |
| `threshold` | Real-world value (after scale division). Compared against `raw / scale` on each tick. |

**Important:** `threshold` is in real-world units, not raw register units.
For a temperature register with `scale: 10`, a threshold of `30.0` means 30.0°C —
not a raw value of 30.

Coils without a `trigger:` are static. They keep their `default` value and can only
be changed by the Modbus client (FC5/FC15 write) or by an `alarm` fault.

**Discrete inputs** follow the same trigger format. They are read-only to Modbus
clients (FC2), but the engine writes their value through the trigger evaluation.

```yaml
discrete:
  - address: 0
    name: unit_on
    default: true
    # no trigger — static read-only status bit

  - address: 1
    name: door_open
    default: false
    trigger:
      source_register: door_sensor_raw
      condition: gt
      threshold: 0.5
```

**Alarm metadata:** The optional top-level `alarms:` section attaches names and severity
levels to coil states. This is purely metadata for display — it does not affect engine
behavior or Modbus register values.

```yaml
alarms:
  - name: "High Temperature"
    severity: warning    # info | warning | critical
    trigger: high_temp_alarm    # name of the coil (not the register)
```

---

## Fault Injection

Faults are temporary overrides injected at runtime via the REST API. They expire
automatically after `duration_s` seconds. Only one fault per register name (or device)
can be active at a time — injecting a second fault for the same register replaces the
first.

**Inject via REST:**

```bash
curl -X POST http://localhost:8000/faults \
  -H "Content-Type: application/json" \
  -d '{
    "fault_type": "spike",
    "register_name": "temperature",
    "value": 45.0,
    "duration_s": 30
  }'
```

**Fields:**

| Field | Type | Description |
| --- | --- | --- |
| `fault_type` | string | One of: `spike`, `freeze`, `dropout`, `noise_amplify`, `alarm` |
| `register_name` | string \| null | Name of the target register or coil. `null` applies to all registers (`dropout` device-wide) |
| `value` | float \| null | Fault parameter. Meaning depends on type (see below) |
| `duration_s` | float | Seconds until the fault expires and normal simulation resumes |

**Manage faults:**

```bash
# List active faults with remaining TTL
curl http://localhost:8000/faults

# Clear all faults immediately
curl -X DELETE http://localhost:8000/faults
```

---

### spike

Forces a register to a specific real-world value for the duration, overriding normal
behavior computation. When the fault expires, the register returns to the behavior
output on the next tick.

```bash
# Force temperature to 45.0°C for 60 seconds
curl -X POST http://localhost:8000/faults \
  -d '{
    "fault_type": "spike",
    "register_name": "temperature",
    "value": 45.0,
    "duration_s": 60
  }'
```

| Field | Value |
| --- | --- |
| `register_name` | Name of any holding register |
| `value` | Target real-world value (before scale) |

**Use cases:**
- Trigger a specific alarm to test downstream SCADA logic
- Push a value to an exact threshold to verify alarm hysteresis
- Simulate a sensor producing a reading beyond its physical range

---

### freeze

Holds a register at its current raw value — the register appears stuck. The behavior
function is still called internally, but the output is replaced by whatever value was
in the store when the fault was applied.

```bash
# Freeze battery_soc at its current value for 5 minutes
curl -X POST http://localhost:8000/faults \
  -d '{
    "fault_type": "freeze",
    "register_name": "battery_soc",
    "value": null,
    "duration_s": 300
  }'
```

| Field | Value |
| --- | --- |
| `register_name` | Name of any holding register |
| `value` | Not used — pass `null` |

**Use cases:**
- Simulate a stuck sensor (common failure mode)
- Test that SCADA detects stale / non-changing values
- Hold a value constant while testing other parts of the system

---

### dropout

Sets a register to `0` — simulating complete loss of signal. Pass `register_name: null`
to drop all holding registers simultaneously (full device communication loss).

```bash
# Single register dropout
curl -X POST http://localhost:8000/faults \
  -d '{
    "fault_type": "dropout",
    "register_name": "input_voltage",
    "value": null,
    "duration_s": 10
  }'

# Device-wide dropout — all registers go to 0
curl -X POST http://localhost:8000/faults \
  -d '{
    "fault_type": "dropout",
    "register_name": null,
    "value": null,
    "duration_s": 15
  }'
```

| Field | Value |
| --- | --- |
| `register_name` | Register name for single dropout, `null` for device-wide |
| `value` | Not used — pass `null` |

**Use cases:**
- Test SCADA handling of communication loss
- Verify that `0` values don't incorrectly trigger alarms (e.g., temperature reads 0°C)
- Test watchdog and timeout logic in polling clients
- Simulate a sensor that lost power

---

### noise\_amplify

Multiplies the `std_dev` of the current behavior by `value` on each tick for the
duration. The register stays "alive" but becomes erratic. Only meaningful for
`gaussian_noise` registers — on other behavior types it applies Gaussian noise using
the amplified `std_dev` around the computed value.

```bash
# Make temperature sensor very noisy for 2 minutes
curl -X POST http://localhost:8000/faults \
  -d '{
    "fault_type": "noise_amplify",
    "register_name": "temperature",
    "value": 20.0,
    "duration_s": 120
  }'
```

| Field | Value |
| --- | --- |
| `register_name` | Name of any holding register |
| `value` | Multiplier for the noise standard deviation. `10.0` = 10× noisier |

**Choosing the multiplier:**
- `2.0` — slightly degraded sensor (barely noticeable)
- `5.0` — clearly unstable sensor, occasional outliers
- `20.0` — severely degraded, highly erratic readings
- `100.0` — extreme noise, values essentially random within register range

**Use cases:**
- Test SCADA alarm filtering and debounce logic
- Simulate a sensor with a loose connection
- Verify that noise filtering doesn't suppress real alarms
- Test operator response to "noisy" readings vs. genuine spikes

---

### alarm

Forces a named **coil** to `True` for the duration, bypassing the normal trigger
evaluation. The holding register values are not affected — only the coil state is
overridden. When the fault expires, the coil reverts to normal trigger-based evaluation.

```bash
# Force high_temp_alarm coil active for 45 seconds
# (temperature register stays at its normal value — alarm fires "without cause")
curl -X POST http://localhost:8000/faults \
  -d '{
    "fault_type": "alarm",
    "register_name": "high_temp_alarm",
    "value": null,
    "duration_s": 45
  }'
```

| Field | Value |
| --- | --- |
| `register_name` | Name of a **coil** (not a register) |
| `value` | Not used — pass `null` |

**Why this is different from `spike`:**
- `spike` raises the register value above the trigger threshold, which causes the coil
  to fire through normal evaluation. Use it when the test requires a realistic causal
  chain (sensor reads high → alarm fires).
- `alarm` forces the coil directly without touching the register. Use it when you want
  to test the downstream SCADA response to the alarm bit itself, independent of what
  the sensor reads.

**Use cases:**
- Test SCADA alarm acknowledgement workflows
- Verify alarm journal entries and notifications
- Test alarm priority and suppression logic
- Simulate a coil forced by an external system (e.g., manual alarm test button)

---

## Practical Recipes

### Recipe 1 — Test a high-temperature alarm in Ignition

```bash
# 1. Confirm the alarm is currently clear
curl http://localhost:8000/registers
# coils: {"0": false}

# 2. Spike temperature above the 30°C threshold for 60 seconds
curl -X POST http://localhost:8000/faults \
  -d '{"fault_type":"spike","register_name":"temperature","value":35.0,"duration_s":60}'

# 3. In Ignition: verify the alarm fires in the Alarm Journal

# 4. After 60 seconds, fault expires — verify alarm auto-clears
curl http://localhost:8000/faults   # should return []
```

---

### Recipe 2 — Simulate UPS discharge and low-battery alarm

```bash
# 1. Use drift behavior (already configured) or accelerate with a direct PATCH
# Set battery_soc to 25% (raw = 250 for scale 10)
curl -X PATCH http://localhost:8000/registers/0 \
  -d '{"value": 250}'

# 2. Let drift run — battery_soc drifts toward 0
# OR force it past the 20% alarm threshold immediately
curl -X PATCH http://localhost:8000/registers/0 \
  -d '{"value": 190}'    # 19% → below threshold → low_battery_alarm fires

# 3. Restore
curl -X POST http://localhost:8000/simulation/reset
```

---

### Recipe 3 — Verify SCADA handles communication loss

```bash
# Drop all registers to 0 for 30 seconds
curl -X POST http://localhost:8000/faults \
  -d '{"fault_type":"dropout","register_name":null,"value":null,"duration_s":30}'

# Confirm the SCADA system raises a "device offline" or "stale data" alert
# After 30s the fault expires and registers return to normal simulation
```

---

### Recipe 4 — Test alarm debounce with noisy sensor

```bash
# Amplify temperature noise by 50× for 2 minutes
# Values will swing wildly — verify SCADA doesn't fire an alarm on transient spikes
curl -X POST http://localhost:8000/faults \
  -d '{"fault_type":"noise_amplify","register_name":"temperature","value":50.0,"duration_s":120}'
```

---

### Recipe 5 — Reproducible test run with a fixed seed

Set `SIMBUS_SEED=42` (or any integer) via environment variable or `.env`.
The RNG is seeded once at startup — every run produces the same sequence of
`gaussian_noise` values, making test assertions deterministic.

```bash
docker run -e SIMBUS_SEED=42 -e SIMBUS_DEVICE_TYPE=generic-tnh-sensor \
  -p 5020:5020 -p 8000:8000 simbus:latest
```

---

### Recipe 6 — Observe live register values while injecting faults

```bash
# Terminal 1 — subscribe to the SSE stream
curl -N http://localhost:8000/registers/stream

# Terminal 2 — inject faults and watch the stream react in real time
curl -X POST http://localhost:8000/faults \
  -d '{"fault_type":"spike","register_name":"temperature","value":45.0,"duration_s":30}'
```

---

### Recipe 7 — Step behavior as a scripted scenario

Use the `step` behavior to pre-script a device lifecycle without any API calls during
the test. The scenario runs automatically from `elapsed_s = 0` on each `reset`.

```yaml
# UPS going through: normal → on-battery → low-battery → shutdown
registers:
  holding:
    - address: 0
      name: battery_soc
      default: 100.0
      scale: 10
      simulation:
        behavior: step
        steps:
          - at: 0      value: 100.0   # fully charged
          - at: 60     value: 80.0    # power fails, on battery
          - at: 120    value: 40.0    # 40% remaining
          - at: 180    value: 15.0    # low battery — alarm fires (threshold 20%)
          - at: 240    value: 5.0     # critical
```

After each test call `POST /simulation/reset` to rewind `elapsed_s` to `0` and run
the sequence again.
