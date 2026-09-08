# runtime

Binary entrypoint (`simbus`). Process contract: [`docs/runtime.md`](../../docs/runtime.md).
Shape of the process: [`docs/architecture.md`](../../docs/architecture.md).

One process loads one device YAML (`--file` / `SIMBUS_YAML_PATH`), or the
official default template when none is given, then starts the tick loop,
Modbus TCP, and the HTTP control API.

`dt = tick_interval × time_scale`. SIGINT/SIGTERM drain HTTP/Modbus accept
loops up to `--shutdown-timeout`, then abort leftover tasks (including SSE).

```bash
simbus
simbus --file devices/builtin/generic-ups.yaml
simbus --time-scale 60 --tick-health 15
simbus check devices/community/papouch-th2e.yaml
simbus ctl status
```

## Tests

```bash
cargo test -p runtime --locked
```

- Unit: clap parsing for `--file`, `--tick`, `--time-scale`, `--seed`, `check`, `ctl`
- Unit: default template loads when `--file` is omitted
