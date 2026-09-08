# runtime

Binary entrypoint (`simbus`). Process contract: [`docs/runtime.md`](../../docs/runtime.md).
Shape of the process: [`docs/architecture.md`](../../docs/architecture.md).

One process loads one device YAML (`--file` / `SIMBUS_YAML_PATH`), or the
official default template when none is given, then starts the tick loop,
Modbus TCP, and the HTTP control API.

```bash
simbus
simbus --file devices/builtin/generic-ups.yaml
simbus check devices/community/papouch-th2e.yaml
```

## Tests

```bash
cargo test -p runtime
```

- Unit: clap parsing for `--file`, `--tick`, `--seed`, `check`
- Unit: default template loads when `--file` is omitted
