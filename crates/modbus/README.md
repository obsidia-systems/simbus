# modbus

Modbus TCP field plane for one `engine` device. Contract: [`docs/modbus.md`](../../docs/modbus.md).

FC1–FC4 reads (holes are `0` / `false`). FC5/FC6/FC15/FC16 writes are
all-or-nothing (`IllegalDataAddress` on holes or a partial multi-word cell).
The MBAP unit id must match YAML `unit_id`; a mismatch is ignored (`Ok(None)`).
RTU / TLS are specified in [`docs/spec.md`](../../docs/spec.md) and not served.

## Tests

```bash
cargo test -p modbus --locked
```

- FC3 T&H defaults (225 / 450); FC6 `update_base`; FC16 adjacent uint16 and float32
- Illegal address (hole, mid-float, extra coil); quantity 0; unit-id filter
