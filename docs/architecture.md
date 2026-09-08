# Architecture

**Mode:** explanation (not a second copy of the YAML or HTTP tables)  
**Contracts:** [spec.md](spec.md) · [runtime.md](runtime.md) · [modbus.md](modbus.md) · [control.md](control.md) · [simulation.md](simulation.md)

simbus is one **process** that pretends to be one **field device**. A lab
is many processes (Compose services), not one process with many maps.

GitHub renders the Mermaid below. Syntax follows current
[Mermaid flowcharts](https://mermaid.js.org/syntax/flowchart.html),
[sequence diagrams](https://mermaid.js.org/syntax/sequenceDiagram.html), and
[state diagrams](https://mermaid.js.org/syntax/stateDiagram.html), without
`%%{init}%%` (GitHub does not apply those directives).

---

## 1. Context

Operators boot a YAML file. SCADA talks Modbus TCP to the listen port.
Tests and GUIs talk HTTP to the control port. The two planes share one
in-memory bank; they are not two devices.

```mermaid
flowchart LR
    subgraph lab [Laboratory]
        scada[SCADA / Ignition]
        gui[GUI / curl / CI]
        subgraph proc [One simbus process]
            mb[Modbus TCP]
            http[HTTP control]
            bank[(RegisterBank)]
            mb --- bank
            http --- bank
        end
    end
    scada -->|FC1 to FC16| mb
    gui -->|REST and SSE| http
```

YAML `type:` is identity inside the document. It is not a CLI selector.
The operator always points `--file` (or `SIMBUS_YAML_PATH`) at a path, or
accepts the default template.

---

## 2. Crates and repository

Compile-time dependencies. `spec` has no tokio. `engine` has no sockets.
`runtime` is the binary `simbus`.

```mermaid
flowchart TB
    runtime[runtime / binary simbus]
    control[control]
    modbusCrate[modbus]
    engine[engine]
    spec[spec]
    runtime --> control
    runtime --> modbusCrate
    runtime --> engine
    control --> engine
    modbusCrate --> engine
    engine --> spec
    control --> spec
    modbusCrate --> spec
```

| Crate | Owns | Must not |
| --- | --- | --- |
| `spec` | YAML parse and `simbus check` | Open ports, tick |
| `engine` | Bank, `tick(dt)`, faults, steps | Sockets, SSE, health logs |
| `modbus` | TCP slave, V1.1b3 PDU | HTTP, tick |
| `control` | Session HTTP + SSE | Modbus listen |
| `runtime` | CLI, boot, three tasks, signals | A second YAML dialect |

CI is `cargo test --workspace --locked`. Per crate (no Markdown next to the
crate; contracts are in this folder):

| Crate | Command | Tests |
| --- | --- | --- |
| `spec` | `cargo test -p spec` | `src/types.rs`; `tests/catalog.rs` (schema fixtures, not every YAML) |
| `engine` | `cargo test -p engine` | `src/behaviors.rs`, `src/encode.rs`; `tests/engine.rs` |
| `control` | `cargo test -p control` | `tests/http.rs` (probes, PATCH, faults, scenarios, API key, SSE) |
| `modbus` | `cargo test -p modbus --locked` | FC1–FC4 / 5 / 6 / 15 / 16 and exceptions 02 / 03 |
| `runtime` (`-p simbus`) | `cargo test -p simbus --locked` | clap (`--file`, `--tick`, `--time-scale`, `--seed`, `check`, `ctl`); default template |

Device YAML is validated with `simbus check`, not a Rust test per file.

On disk this is the whole product (no Python package, no `scenarios/` folder):

```text
simbus/
├── crates/{spec,engine,control,modbus,runtime}
├── devices/{builtin,community}
├── docs/
├── Dockerfile
├── docker-compose.yml
└── Cargo.toml
```

---

## 3. Boot

File-only load. `simbus check` stops after validation. Boot refuses
unimplemented protocol bindings that `check` still accepts as syntax.

```mermaid
flowchart TB
    start([simbus argv]) --> parse[Parse CLI and SIMBUS_*]
    parse --> isCheck{subcommand?}
    isCheck -->|check| loadCheck[Load explicit path]
    loadCheck --> validate
    isCheck -->|ctl| client[HTTP client, no boot]
    isCheck -->|none| loadBoot[Load file, cwd default.yaml, or embedded]
    loadBoot --> validate[spec validate]
    validate --> unimplemented{unimplemented binding?}
    unimplemented -->|check| report[Print summary, exit 0]
    unimplemented -->|boot| die[Exit error]
    unimplemented -->|none, check| report
    unimplemented -->|none, boot| device[Device::new]
    device --> running[set_running true]
    running --> spawn[Spawn tick, Modbus, HTTP]
    spawn --> log[Log simbus started]
    log --> wait[Wait for signal or task death]
```

Sequence of the three tasks after `Device::new`:

```mermaid
sequenceDiagram
    autonumber
    actor Op as Operator
    participant RT as runtime
    participant Eng as engine
    participant MB as modbus
    participant CTL as control
    Op->>RT: simbus --file device.yaml
    RT->>Eng: Device::new spec, seed, tick
    RT->>MB: spawn serve port, unit_id
    RT->>CTL: spawn serve host, api_port
    RT->>RT: log simbus started
    Note over MB,CTL: Tick loop publishes Snapshot on watch after every tick dt
```

---

## 4. Shared bank

There is one `RegisterBank`. Tick writes it. Modbus and HTTP read and
write it. SSE is a `watch` of snapshots; the engine never opens the SSE
socket.

```mermaid
flowchart LR
    tick[tick dt] --> bank[(RegisterBank)]
    mb[Modbus FC3 / FC6] --> bank
    patch[PATCH /registers] --> bank
    bank --> sse[watch channel]
    sse --> stream[GET /registers/stream]
```

---

## 5. Field plane vs session plane

| Plane | Port | Client | Contract |
| --- | --- | --- | --- |
| Field | Modbus TCP | Ignition, PLC, protocol tester | [modbus.md](modbus.md) |
| Session | HTTP | Operator, GUI, pytest | [control.md](control.md) |

A FC6 is not an HTTP PATCH. `state.base` updates in both cases
([simulation.md](simulation.md) §3). SSE publishes immediately on session
writes and on the **next tick** after a Modbus write.

---

## 6. Process lifecycle

`is_running` is the pause flag (`PATCH /simulation` `running`). Tick is a
no-op while paused. `/readyz` is 503 while paused. Modbus still serves the
last bank.

```mermaid
stateDiagram-v2
    [*] --> Booting
    Booting --> Running: servers listening
    Running --> Paused: PATCH running false
    Paused --> Running: PATCH running true
    Running --> Stopping: SIGINT or SIGTERM
    Paused --> Stopping: SIGINT or SIGTERM
    Running --> Failed: Modbus or HTTP task died
    Paused --> Failed: Modbus or HTTP task died
    Stopping --> [*]: exit 0
    Failed --> [*]: exit non-zero
```

Shutdown **drains** then aborts leftover after `--shutdown-timeout`
([runtime.md](runtime.md) §6).

---

## 7. Docker lab

Each Compose service is one process, one YAML, one pair of published
ports. Inside the container Modbus is usually `502` (Papouch `512`).
Compose **must** set `SIMBUS_YAML_PATH`.

Every service has a profile (`env`, `cooling`, `power`, `custom`, `all`).
`docker compose up` with no names and no `--profile` starts nothing.
`--profile all` starts 7 builtin templates plus 4 Papouch TH2E instances.
Naming a service (`docker compose up tnh-sensor`) starts it without
enabling a profile.

```mermaid
flowchart TB
    subgraph host [Host]
        ign[Ignition]
        subgraph compose [docker compose]
            tnh[tnh-sensor]
            ups[ups]
            pdu[pdu]
        end
    end
    ign -->|Modbus host ports| tnh
    ign --> ups
    ign --> pdu
```

---

## 8. What this page is not

- YAML field tables → [spec.md](spec.md)
- Function codes and exception 02 → [modbus.md](modbus.md)
- Route list → [control.md](control.md) and `GET /docs`
- Behavior formulas → [simulation.md](simulation.md)
