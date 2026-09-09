# Architecture

**Mode:** explanation (not a second copy of the YAML or HTTP tables)  
**Contracts:** [spec.md](spec.md) · [runtime.md](runtime.md) · [modbus.md](modbus.md) · [opcua.md](opcua.md) · [bacnet.md](bacnet.md) · [control.md](control.md) · [simulation.md](simulation.md)

simbus is one **process** that pretends to be one **field device**. A lab
is many processes (Compose services), not one process with many maps.

GitHub renders the Mermaid below. Syntax follows current
[Mermaid flowcharts](https://mermaid.js.org/syntax/flowchart.html),
[sequence diagrams](https://mermaid.js.org/syntax/sequenceDiagram.html), and
[state diagrams](https://mermaid.js.org/syntax/stateDiagram.html), without
`%%{init}%%` (GitHub does not apply those directives).

---

## 1. Context

Operators boot a YAML file. SCADA talks Modbus TCP (and optionally TLS on
IANA 802, OPC UA on IANA 4840, and/or BACnet/IP on IANA 47808) to the listen
ports. Tests and GUIs talk HTTP to the control port. The planes share one
in-memory bank; they are not several devices.

```mermaid
flowchart LR
    subgraph lab [Laboratory]
        scada[SCADA / Ignition / BMS]
        gui[GUI / curl / CI]
        subgraph proc [One simbus process]
            mb[Modbus TCP / TLS]
            ua[OPC UA]
            bn[BACnet/IP]
            http[HTTP control]
            bank[(RegisterBank)]
            mb --- bank
            ua --- bank
            bn --- bank
            http --- bank
        end
    end
    scada -->|FC1 to FC16 / UA read| mb
    scada --> ua
    scada -->|Who-Is / RP / WP| bn
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
    opcuaCrate[opcua]
    bacnetCrate[bacnet]
    engine[engine]
    spec[spec]
    runtime --> control
    runtime --> modbusCrate
    runtime --> opcuaCrate
    runtime --> bacnetCrate
    runtime --> engine
    control --> engine
    modbusCrate --> engine
    opcuaCrate --> engine
    bacnetCrate --> engine
    engine --> spec
    control --> spec
    modbusCrate --> spec
    opcuaCrate --> spec
    bacnetCrate --> spec
```

| Crate | Owns | Must not |
| --- | --- | --- |
| `spec` | YAML parse and `simbus check` | Open ports, tick |
| `engine` | Bank, `tick(dt)`, faults, steps | Sockets, SSE, health logs |
| `modbus` | TCP / TLS slave, V1.1b3 PDU | HTTP, tick |
| `opcua` | OPC UA server, YAML map as variables | HTTP, tick |
| `bacnet` | BACnet/IP server, objects from `export` | HTTP, tick |
| `control` | Session HTTP + SSE | Field listen |
| `runtime` | CLI, boot, tick / field listeners / HTTP, signals | A second YAML dialect |

CI is `cargo test --workspace --locked`. Per crate (no Markdown next to the
crate; contracts are in this folder):

| Crate | Command | Tests |
| --- | --- | --- |
| `spec` | `cargo test -p spec` | `src/types.rs`; `tests/catalog.rs` (schema fixtures, not every YAML) |
| `engine` | `cargo test -p engine` | `src/behaviors.rs`, `src/encode.rs`; `tests/engine.rs` |
| `control` | `cargo test -p control` | `tests/http.rs` (probes, PATCH, faults, scenarios, API key, SSE) |
| `modbus` | `cargo test -p modbus --locked` | FC1–FC4 / 5 / 6 / 15 / 16, exceptions 02 / 03, FC3 over TLS |
| `opcua` | `cargo test -p opcua --locked` | T&H `holding/temperature` over Anonymous/None |
| `bacnet` | `cargo test -p bacnet --locked` | Language-2 fixture: Who-Is, ReadProperty on an Analog Input, WriteProperty on an Analog Value |
| `runtime` (`-p simbus`) | `cargo test -p simbus --locked` | clap (`--file`, `--tick`, `--time-scale`, `--seed`, TLS/UA flags, `check`, `ctl`); default template |

Device YAML is validated with `simbus check`, not a Rust test per file.

On disk this is the whole product (no Python package, no `scenarios/` folder):

```text
simbus/
├── crates/{spec,engine,control,modbus,opcua,bacnet,runtime}
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
    running --> spawn[Spawn tick, field listeners, HTTP]
    spawn --> log[Log simbus started]
    log --> wait[Wait for signal or task death]
```

Sequence of tick, field listeners, and HTTP after `Device::new`:

```mermaid
sequenceDiagram
    autonumber
    actor Op as Operator
    participant RT as runtime
    participant Eng as engine
    participant MB as modbus
    participant UA as opcua
    participant BN as bacnet
    participant CTL as control
    Op->>RT: simbus --file device.yaml
    RT->>Eng: Device::new spec, seed, tick
    RT->>MB: spawn serve and/or serve_tls if YAML asked
    RT->>UA: spawn serve if YAML asked
    RT->>BN: spawn serve if YAML asked
    RT->>CTL: spawn serve host, api_port
    RT->>RT: log simbus started
    Note over MB,CTL: Tick loop publishes Snapshot on watch after every tick dt
```

---

## 4. Shared bank

There is one `RegisterBank`. Tick writes it. Modbus, OPC UA, BACnet/IP, and
HTTP read and write it. SSE is a `watch` of snapshots; the engine never opens
the SSE socket.

```mermaid
flowchart LR
    tick[tick dt] --> bank[(RegisterBank)]
    mb[Modbus FC3 / FC6] --> bank
    ua[OPC UA read / write] --> bank
    bn[BACnet RP / WP] --> bank
    patch[PATCH /registers or /points] --> bank
    bank --> sse[watch channel]
    sse --> stream["GET /registers/stream or /points/stream"]
```

---

## 5. Field plane vs session plane

| Plane | Port | Client | Contract |
| --- | --- | --- | --- |
| Field | Modbus TCP / TLS | Ignition, PLC, protocol tester | [modbus.md](modbus.md) |
| Field | OPC UA (IANA 4840) | Ignition, UA Expert | [opcua.md](opcua.md) |
| Field | BACnet/IP (IANA 47808) | BMS, YABE, protocol tester | [bacnet.md](bacnet.md) |
| Session | HTTP | Operator, GUI, pytest | [control.md](control.md) |

A FC6, an OPC UA write, a BACnet WriteProperty, and an HTTP PATCH are wires to the same
`state.base` ([simulation.md](simulation.md) §3). SSE publishes immediately
on session writes and on the **next tick** after a field-plane write.

---

## 6. Process lifecycle

`is_running` is the pause flag (`PATCH /simulation` `running`). Tick is a
no-op while paused. `/readyz` is 503 while paused. Field listeners still
serve the last bank.

```mermaid
stateDiagram-v2
    [*] --> Booting
    Booting --> Running: servers listening
    Running --> Paused: PATCH running false
    Paused --> Running: PATCH running true
    Running --> Stopping: SIGINT or SIGTERM
    Paused --> Stopping: SIGINT or SIGTERM
    Running --> Failed: field listener or HTTP task died
    Paused --> Failed: field listener or HTTP task died
    Stopping --> [*]: exit 0
    Failed --> [*]: exit non-zero
```

Shutdown **drains** then aborts leftover after `--shutdown-timeout`
([runtime.md](runtime.md) §6).

---

## 7. Docker lab

Each Compose service is one process, one YAML, one pair of published
ports (Modbus + HTTP). Official maps do not bind OPC UA or BACnet/IP; a lab
that wants IANA 4840 adds `protocol: opcua` and publishes `4840`, and one that
wants IANA 47808 adds `protocol: bacnet-ip` and publishes `47808/udp`. Inside the
container Modbus is usually `502` (Papouch `512`). Compose **must** set
`SIMBUS_YAML_PATH`.

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
    ign -->|optional OPC UA 4840| tnh
    ign --> ups
    ign --> pdu
```

---

## 8. What this page is not

- YAML field tables → [spec.md](spec.md)
- Function codes and exception 02 → [modbus.md](modbus.md)
- OPC UA NodeIds and None/Anonymous → [opcua.md](opcua.md)
- BACnet object types and Present_Value → [bacnet.md](bacnet.md)
- Route list → [control.md](control.md) and `GET /docs`
- Behavior formulas → [simulation.md](simulation.md)
