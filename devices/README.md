# Device maps

simbus is a generic measurement engine. Official maps are **templates**: you
point `simbus --file` at one of them. Vendor or site-specific maps live in
`community/` and are accepted by pull request.

| Folder | Who maintains it | What belongs here |
| --- | --- | --- |
| `builtin/default.yaml` | Obsidia | Zero-arg default: example channels, not a product |
| `builtin/` | Obsidia | Product-shaped templates (T&H, UPS, PDU, CRAC, …) |
| `community/` | Contributors | Named products, real register maps, lab-specific YAML |

```bash
simbus                                          # default.yaml
simbus --file devices/builtin/generic-ups.yaml
simbus --file devices/community/papouch-th2e.yaml
simbus check devices/community/my-device.yaml
```

Bundled scenarios belong **in the YAML** (`scenarios:`). See [docs/spec.md](../docs/spec.md).

Exit `0` from `check` prints a configuration summary. Exit `1` prints the
validation error. There is no Rust test per YAML file; CI runs `simbus check`
on every file under `devices/`.
