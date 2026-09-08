# modbus

Modbus TCP field plane for one `engine` device. Contract:
[`docs/modbus.md`](../../docs/modbus.md) (V1.1b3 PDU + V1.0b MBAP).

FC1–FC4 / 5 / 6 / 15 / 16. A range that includes an unimplemented address
is exception 02. Quantity 0 or above the spec max is exception 03. Each PDU
address is a 16-bit register. Native TCP does not filter on unit id.
`serve` stops accepting when the runtime shutdown future resolves
(`serve_until`). RTU / TLS are specified in [`docs/spec.md`](../../docs/spec.md)
and not served.

## Tests

```bash
cargo test -p modbus --locked
```

- FC3 T&H defaults; FC3 past the map → 02; FC6 `update_base`; FC16 adjacent uint16
- FC6 splices one word of a float32 pair; quantity 0 → 03; extra coil → 02
