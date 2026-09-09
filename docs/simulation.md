# Simulation Engine

**Status:** normative for `crates/engine` (language versions 1 and 2)  
**Device language:** [spec.md](spec.md)  
**Process:** [runtime.md](runtime.md)  
**HTTP session:** [control.md](control.md)  
**Field plane:** [modbus.md](modbus.md) · [opcua.md](opcua.md)  
**Shape of the process:** [architecture.md](architecture.md)

This document is the contract of **the tick**: time, `state.base`, behaviors,
triggers, faults, encode, and reset. It does not define YAML syntax or HTTP
verbs.

The crate MUST NOT open sockets, publish SSE, or emit process health logs.
`Device::tick(dt)` returns a `Snapshot`. The runtime publishes that snapshot
on a `watch` channel; the control plane streams it ([control.md](control.md)).

---

## 1. Role

The engine owns one in-memory register bank for one loaded document.

On every **tick** with `dt > 0` it MUST:

1. Decrement fault timers by `dt` and drop faults with `remaining_s <= 0`.
2. Advance `elapsed_s` by `dt` for every holding and input register.
3. For each holding and input register that has a `simulation:` block:
   compute a real-world value from `state.base`, apply any matching fault,
   encode, write the cell.
4. Evaluate coil and discrete triggers against current real values.
5. Return a snapshot of the bank.

`dt <= 0` MUST be a no-op (same snapshot, no time, no expiry).
`is_running == false` (pause) MUST be the same no-op even when `dt > 0`.
Resume continues from the frozen `elapsed_s` and fault `remaining_s`.

Registers without `simulation:` are static: they keep `default` or whatever
was last written. The engine MUST NOT overwrite them on tick.

Pause is set from the control plane ([control.md](control.md)). The engine
MUST consult `is_running` inside `tick`. The runtime MUST NOT publish an
SSE tick frame for a skipped (paused) wait.

```mermaid
flowchart TB
    tick[tick dt] --> paused{is_running and dt greater than 0?}
    paused -->|no| snap[Return snapshot]
    paused -->|yes| faults[Decrement fault TTLs]
    faults --> regs[For each holding and input]
    regs --> beh{has simulation?}
    beh -->|no| skip[Keep last written]
    beh -->|yes| compute[Compute from state.base]
    compute --> flt{matching fault?}
    flt -->|yes| over[Override]
    flt -->|no| enc[Encode and write cell]
    over --> enc
    skip --> trig
    enc --> trig[Evaluate triggers]
    trig --> snap
```

---

## 2. Time

`dt` is **simulation seconds**. The runtime sleeps `tick_interval` (wall
seconds) and passes `dt = tick_interval × time_scale`
([runtime.md](runtime.md) §4). Default `time_scale` is `1` (1:1).

`--tick` / `PATCH /simulation` is the sample period. A 12-hour sine still
takes 12 simulation hours. With `--time-scale 60` those 12 simulation hours
elapse in 12 wall minutes. The engine MUST NOT store a second clock;
only the `dt` the caller passes matters.

Periodic behaviors (`sinusoidal`, `square`, `sawtooth`, `triangle`,
`cycle`, `step`) and fault TTLs use elapsed simulation time. Drift uses
**engineering units per simulation second**, applied as `rate × dt`.
Changing `--tick` MUST NOT change the physical trajectory, only how often
it is sampled. Changing `--time-scale` MUST (same trajectory, faster or
slower wall time).

---

## 3. Operating point — `state.base`

Every numeric register has `state.base`, initialized from YAML `default`
(then jittered — §6).

Behaviors that **use** `state.base` as center or drift state: `constant`,
`gaussian_noise`, `sinusoidal`, `square`, `drift`. A PATCH shifts the
operating point and the next tick follows it.

Behaviors that **own** the live value (YAML range, list, or schedule):
`sawtooth`, `triangle`, `uniform`, `cycle`, `step`. `PATCH` / FC6 still
call `update_base`; the value is visible until the next tick, then
overwritten from the waveform, draw, or schedule (§5).

`PATCH /registers/…`, `PATCH /points/{id}`, and Modbus FC6/FC16 MUST call `update_base` for **each**
cell whose words changed: decode `raw / scale` (or the provided `real_value`)
into `state.base`. For behaviors that use `state.base`, the next tick runs
from that point. It MUST NOT snap back to YAML `default`. FC16 of two
adjacent `uint16` cells MUST update both bases. FC6 of one word of a
`float32`/`uint32` pair splices that word into the cell, then updates that
one base ([modbus.md](modbus.md) §3.2).

---

## 4. Tick interval live update

The engine stores `tick_interval`. `set_tick_interval` MUST take effect on
the next wait in the runtime loop. `tick(dt)` uses the `dt` the caller
passes, not a cached copy from construction.

---

## 5. Behaviors

Syntax: [spec.md](spec.md) §6. Computation below. `t` is
`elapsed_s + phase_s` (§6).

### constant

`value = state.base`

### gaussian_noise

If a drift modifier is present and `enabled`, apply it first (§5.1).
Then sample Normal(`state.base`, `std_dev`).

### sinusoidal

```text
value = state.base + amplitude × sin(2π × t / (period_hours × 3600))
```

### square

50% duty, analog high then low around `state.base`:

```text
u = (t mod period_seconds) / period_seconds
value = state.base + amplitude     if u < 0.5
value = state.base − amplitude     otherwise
```

`period_seconds` is the full high+low cycle (lab-scale, unlike
`sinusoidal`'s `period_hours`).

### drift

```text
state.base = clamp(state.base + rate × dt, bounds)
value = state.base
```

`rate` is engineering units **per simulation second**. At the default
`tick_interval=1.0` this matches maps written against “per tick”. It does
not bounce at a bound.

### sawtooth

```text
value = min + (max − min) × (t mod period_seconds) / period_seconds
```

Rising ramp only. Resets to `min` at each period. Does not use
`state.base`.

### triangle

Symmetric ramp `min → max → min` in one `period_seconds`:

```text
u = (t mod period_seconds) / period_seconds
value = min + (max − min) × (2u)           if u < 0.5
value = max − (max − min) × (2u − 1)       otherwise
```

Does not use `state.base`. At `u = 0` the value is `min`; at `u = 0.5` it
is `max`.

### uniform

Each tick, sample Uniform(`min`, `max`) inclusive. Independent of
`state.base`. Distinct from `gaussian_noise` (Normal around the operating
point).

### step

Hold YAML `default` until the first `at`, then the last step with
`at <= t`. Equal `at` values: last in the list wins. The last step is held
indefinitely. PATCH does not change the schedule.

### cycle

Walk YAML `values` in order, one index every `dwell_seconds` of simulation
time, then repeat from the first:

```text
i = floor(t / dwell_seconds) mod len(values)
value = values[i]
```

Does not use `state.base`. Empty `values` is invalid (spec §6). Unlike
`step`, there is no absolute `at` and the walk does not stop at the last
entry.

### 5.1 Drift modifier

Allowed only on `gaussian_noise` and `sinusoidal`. Runs **before** the
behavior:

```text
state.base = clamp(state.base + rate × dt, bounds)
```

`enabled: false` MUST skip the modifier.

---

## 6. Fleet diversity

Two processes MUST NOT replay the same curve just because they share a
numeric `--seed`.

At `Device::new`:

- Mix `--seed` with `name`, `type`, and `identity` (`vendor`, `product`,
  `revision`) via FNV-1a. Unseeded devices use OS entropy.
- `sinusoidal` / `square` / `sawtooth` / `triangle` / `cycle` / `step` get
  a random `phase_s` inside one period (`square`/`sawtooth`/`triangle`:
  `0..period_seconds`; `cycle`: one full walk `0..(dwell_seconds ×
  len(values))`; `step`: `0..60` s).
- `gaussian_noise` jitters `state.base` by one sample of `std_dev`.
- `drift` jitters `state.base` by ~2% of the bound span, then clamps.

The bank still shows YAML defaults until the **first** tick. After that,
values come from the jittered operating point.

`POST /simulation/reset` MUST restore each register’s **boot** `RegState`
(jittered base, original `phase_s`, `elapsed_s = 0`), rewind the bank to
YAML defaults, restore the RNG stream to its post-init state, and clear
faults. It MUST NOT re-roll from the OS. A seeded device MUST replay the
same post-reset trace as after `new`. `tick_interval` is unchanged.

---

## 7. Triggers

After numeric writes, every coil and discrete with `trigger:` is evaluated
from the source register’s **real** value (`raw / scale`).

| condition | meaning |
| --- | --- |
| `gt` / `lt` / `gte` / `lte` | ordinary comparison |
| `eq` | `|value − threshold| ≤ 0.5 / scale` (half a raw LSB) |

`eq` is quantized on purpose: two encodings of the same cell MUST match.

Coils without a trigger stay at `default` unless written (Modbus or API) or
forced by an `alarm` fault. A triggered coil MAY be written, but the next
tick overwrites it.

Top-level `alarms:` is metadata. It MUST NOT change bank values.

---

## 8. Faults

Faults are TTL overrides. Key = register or coil `name`, or `_device` when
`register_name` is omitted. A second inject for the same key replaces the
first.

`duration_s` is simulation seconds. `remaining_s` decreases by `dt`.

Lookup on a numeric register: named fault, else `_device`.

| type | effect |
| --- | --- |
| `spike` | If `value` is set, force that real. Missing `value` is a no-op. Holding **and** input. |
| `freeze` | Latch the cell’s real value **at inject**. Later PATCH/FC6 change `state.base` but the frozen output stays until expiry. Holding and input. |
| `dropout` | Force `0`. Named: one register. `_device`: every holding **and** input cell. |
| `noise_amplify` | After the behavior, add Extra Normal noise with `std_dev × value` (default factor `10`). On non-gaussian behaviors the `std_dev` fallback is `0.5`. |
| `alarm` | Force the **coil** of that name to `true`, skip trigger. Does not change registers. Unknown name: stored, no effect. Discrete is not a target. |

```mermaid
stateDiagram-v2
    [*] --> Live
    Live --> Frozen: inject freeze
    Frozen --> Frozen: PATCH, FC6, or OPC UA write updates base only
    Frozen --> Live: TTL expired
```

When a fault expires, the next tick uses normal behavior / triggers.

---

## 9. Encode

`raw ≈ round(real × scale)`, then clamp to the type range:

| type | clamp |
| --- | --- |
| `uint16` | `[0, 65535]` |
| `int16` | `[i16::MIN, i16::MAX]` |
| `uint32` | `[0, u32::MAX]` |
| `float32` | finite `f32` range |

The engine MUST NOT wrap `uint16` (a spiked temperature MUST NOT become a
small unsigned value). Multi-word cells use document `endianness`.

---

## 10. Scenarios

`Device::apply_step` mutates this device immediately. The runner in this
crate only tracks generation / progress; sleeps live in the HTTP layer
([control.md](control.md)).

Unknown register or coil names in a step MUST be skipped (warn), not panic.

---

## 11. Change process

1. Update this document.
2. Change `crates/engine` (tick, behaviors, encode, faults, reset).
3. Cover the new rule in `crates/engine/tests/engine.rs` or a unit test
   next to the function.
4. YAML catalog files stay validated by `simbus check`, not by enumerating
   them here.

Language syntax still starts in [spec.md](spec.md). HTTP verbs still start
in [control.md](control.md).

---

## 12. Recipes (session)

These assume a running process. They do not extend the tick contract.

**High-temp alarm (generic T&H):**

```bash
curl -X POST http://localhost:8000/faults \
  -d '{"fault_type":"spike","register_name":"temperature","value":35.0,"duration_s":60}'
```

**Dropout the whole map:**

```bash
curl -X POST http://localhost:8000/faults \
  -d '{"fault_type":"dropout","register_name":null,"value":null,"duration_s":30}'
```

**Rewind to boot (same seeded trace):**

```bash
curl -X POST http://localhost:8000/simulation/reset
```

**Watch the bank** (control samples; not a tick callback):

```bash
curl -N http://localhost:8000/registers/stream
```
