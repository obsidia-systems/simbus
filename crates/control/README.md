# control

HTTP **control plane** for one already-booted device. Not named `api`:
Modbus is the field plane; this crate is the operator session (PATCH,
faults, scenarios, SSE). Transport is HTTP. Contract:
[`docs/control.md`](../../docs/control.md).

Writes may be gated by `SIMBUS_API_KEY`. Scenario catalog comes from the
loaded device spec. SSE (`GET /registers/stream`) follows engine ticks via
a `watch` channel filled by the runtime.

## Tests

```bash
cargo test -p control
```

- Integration (`tests/http.rs`): probes, `/status`, `/config` wire format,
  bundled scenarios, 404 unknown scenario, API key, OpenAPI path set, first
  SSE frame
