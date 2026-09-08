# control

HTTP **control plane** for one already-booted device. Not named `api`:
Modbus is the field plane; this crate is the operator session (PATCH,
faults, scenarios, SSE). Transport is HTTP. Contract:
[`docs/control.md`](../../docs/control.md).
Field plane: [`docs/modbus.md`](../../docs/modbus.md).

Writes may be gated by `SIMBUS_API_KEY`. Scenario catalog comes from the
loaded device spec. Scenario `at:` sleeps `at / time_scale` of wall clock.
SSE (`GET /registers/stream`) follows engine ticks via a `watch` channel
filled by the runtime. `/status` includes `time_scale`.

## Tests

```bash
cargo test -p control
```

- Integration (`tests/http.rs`): probes, `/status`, `/config`, PATCH,
  faults 404/422, bundled scenarios, API key, OpenAPI, first SSE frame
