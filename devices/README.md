# Device maps

simbus is a generic measurement engine. Official maps are one generic device
per type. Vendor-specific or site-specific maps live in `community/` and are
accepted by pull request.

| Folder | Who maintains it | What belongs here |
| --- | --- | --- |
| `builtin/` | Obsidia | Generic devices (T&H, UPS, PDU, CRAC, …) |
| `community/` | Contributors | Named products, real register maps, lab-specific YAML |

`--type <name>` looks for `<name>.yaml` in this order:

1. `devices/`
2. `devices/builtin/`
3. `devices/community/`

An extra directory can be prepended with `--devices-dir` / `SIMBUS_DEVICES_DIR`.

Bundled scenarios belong **in the YAML** (`scenarios:`). See [docs/spec.md](../docs/spec.md).

Validate a file without starting Modbus or the API:

```bash
simbus check devices/community/my-device.yaml
```

Exit `0` prints a configuration summary. Exit `1` prints the validation error.
There is no Rust test per YAML file; CI runs `simbus check` on every file under
`devices/`.
