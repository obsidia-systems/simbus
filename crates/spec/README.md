# spec

Parse and validate the simbus device language. No tokio, no protocol stacks.

Normative syntax: [`docs/spec.md`](../../docs/spec.md).

`--type` resolution and the `simbus check` report live here. New protocols are
added as YAML bindings here first (`ProtocolId::is_implemented` stays false
until a runtime actually serves them).

## Tests

```bash
cargo test -p spec
```

- Unit: YAML wire strings in `src/types.rs`
- Integration (`tests/catalog.rs`): schema fixtures, overlap/trigger rejection,
  embedded scenarios, unimplemented bindings, `--type` search paths

Catalog YAML files are not enumerated in Rust tests. Validate them with
`simbus check <file>`; CI runs that over `devices/**/*.yaml`.
