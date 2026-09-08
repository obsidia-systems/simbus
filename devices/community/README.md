# Community device maps

Add a YAML here and open a pull request. Official generic types stay in
`../builtin/`.

Before you open a PR:

```bash
cargo run -p runtime -- check devices/community/your-device.yaml
```

A valid file prints `OK` plus the name, type, Modbus binding, register counts,
behaviors, triggers, alarms, and bundled scenarios. Put demos under `scenarios:`
in the same file ([docs/spec.md](../../docs/spec.md)). An invalid file exits `1`.

Do not add a Rust test for each map. CI already runs `simbus check` on every
YAML under `devices/`.
