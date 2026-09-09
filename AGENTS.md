# simbus

Instructions for coding agents working **in this repository** (build, tests,
contracts). Not a device-authoring playbook — that is
`.agents/skills/simbus-device/SKILL.md` ([Agent Skills](https://agentskills.io/home)).
Documentation map: `llms.txt`.

## Product

Industrial Modbus TCP/TLS, OPC UA, and BACnet/IP field-device simulator.
**One process = one device.** Binary name `simbus`. Crates: `spec`, `engine`,
`control`, `modbus`, `opcua`, `bacnet`, `runtime`.

**Version in this tree: `0.3.0`** (`[workspace.package]` in `Cargo.toml`).
That is the version we are working on (unreleased until tagged). Do not
invent a different semver in docs or crates.

**Rust:** there is **no official LTS**. `rust-version = "1.85"` is the
**MSRV** (floor for edition 2024). `rust-toolchain.toml` is `channel = "stable"`
(CI uses the current stable). Edition **2024** is the language edition, not
the calendar year. Caveat: the `bacnet-server` stack declares its own floor of
**1.93**, so `crates/bacnet` does not build on a 1.85 toolchain. Do not raise
`rust-version` for it (`docs/bacnet.md` § Gaps).

The device YAML is the boot contract (`docs/spec.md`). Session mutations are
HTTP (`docs/control.md`). Modbus TCP/TLS (IANA 802), OPC UA (IANA 4840), and
BACnet/IP (IANA 47808) are the field plane (`docs/modbus.md`, `docs/opcua.md`,
`docs/bacnet.md`).

There is no Python tree, no `scenarios/` folder, no `--type` selector.

## Agent skill (device YAML)

Canonical folder: `.agents/skills/simbus-device/` (a skill is a directory with
`SKILL.md`; it does **not** need its own git repo). Do not put `SKILL.md` at
the repo root (the Skills CLI would treat the whole product as one skill).
`.cursor/` is local editor state (gitignored), not the skill home.

In this clone, agents load it as a project skill. Into another workspace:

```bash
npx skills add obsidia-systems/simbus@simbus-device
```

## Keep docs and agent surfaces in sync

When an implementation changes a contract, CLI, HTTP route, device map, or
authoring workflow, update **in the same change** every surface that still
applies:

- Normative docs in `docs/`. If the change moves a number, a frame, or a
  timeline that a diagram shows, redraw it in the same change — allowed
  diagram types and the Mermaid rules are in `docs/README.md` § Diagrams
- `README.md` if the front door would become wrong
- `CONTRIBUTING.md` if the branch model or PR target changed
- `CHANGELOG.md` (`## [Unreleased]`). On a **release**, also bump every row
  under [Release](#release) (Cargo workspace, this file, CHANGELOG heading,
  README roadmap if it names the current series)
- `AGENTS.md` if build/test/constraints for agents changed
- `llms.txt` if the documentation map changed
- `.agents/skills/simbus-device/SKILL.md` if how to write a device YAML changed

Do not leave a skill or `llms.txt` describing a flag, folder, or protocol
that the code no longer has. Skip a file only when it is truly unaffected.

## Commands

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo run -p simbus -- check devices/community/papouch-th2e.yaml
cargo run -p simbus -- --file devices/builtin/generic-tnh-sensor.yaml
```

CI is those five (fmt, clippy, test, `cargo deny`, `simbus check` on every
`devices/**/*.yaml`). Do not add a Rust test per device YAML. GitHub runs
that **CI** workflow on PRs and pushes to `develop` / `main`. Release
(`dist`) and GHCR do not run on feature PRs: `dist plan` and a Docker
build-only check run on the `develop` → `main` PR; a `v*` tag on `main`
publishes.

## Spec-first

| Change | Write first |
| --- | --- |
| YAML language / new protocol syntax | `docs/spec.md` + `crates/spec` |
| Process, CLI, signals, Docker | `docs/runtime.md` + `crates/runtime` |
| HTTP routes | `docs/control.md` + `crates/control` |
| Tick / behaviors / faults | `docs/simulation.md` + `crates/engine` |
| Modbus PDU / exceptions | `docs/modbus.md` + `crates/modbus` |
| OPC UA address space / endpoint | `docs/opcua.md` + `crates/opcua` |
| BACnet objects / APDU | `docs/bacnet.md` + `crates/bacnet` |
| Community map only | YAML + `simbus check` (no crate change) |
| Agent/docs surfaces | `docs/`, `AGENTS.md`, `llms.txt`, skill — see above |

When a contract and the Rust types disagree, the contract wins — change them
together. New code, comments, and docs are English.

## Architecture constraints

- `spec`: no tokio, no sockets.
- `engine`: no sockets, no SSE, no process health logs. `tick(dt)` only.
- `modbus`: field plane. Follow industry Modbus (V1.1b3 / V1.0b); TLS is a wrap
  of that PDU (IANA 802), not a second protocol.
- `opcua`: field plane. Same YAML bank as Modbus; None + Anonymous in this
  version (IANA 4840).
- `bacnet`: field plane. Objects come from the language-2 `export` of that
  binding; Who-Is/I-Am, ReadProperty, WriteProperty on `Present_Value`
  (IANA 47808). No BACnet/SC, COV, or BBMD in this version.
- `control`: session HTTP. Must not enlarge the register map.
- `runtime`: one binary, file-only boot (`--file` / `SIMBUS_YAML_PATH`, else
  `devices/builtin/default.yaml` or embedded). `simbus check` needs an explicit path.
- Official maps: `devices/builtin/`. Contributor maps: `devices/community/` (PR).
- Scenarios live **in** the device YAML (`scenarios:`), kebab-case `id`.
- `type:` is identity inside the document, not a CLI flag.
- Unimplemented protocol **syntax** is valid (`check` notes it). Boot refuses.

## Deferred

`docs/debt.md`: serving extra protocols (spec §4). Do not restore Python.

## Branches

Simplified GitFlow. Human guide: [CONTRIBUTING.md](CONTRIBUTING.md).

- Open PRs to **`develop`**, not `main`.
- Do not commit to `main` (release PR from `develop` only).
- Do not tag `develop` or a feature branch. Tag `vX.Y.Z` only on `main`.
- GitHub default branch should be `develop`.

## Commits

Do not commit unless the user asks. Do not force-push. Do not skip hooks.

## Release

Crate / binary version is **not** YAML `spec_version` (language) and **not**
a device file's `version:` (map). Those stay unless the language or that map
actually changes.

### Version locations (keep them identical)

Bump **all** of these together. Search the tree for the old `x.y.z` after.

| Place | What |
| --- | --- |
| `Cargo.toml` `[workspace.package] version` | Source of truth (`simbus --version`, crate versions) |
| `Cargo.toml` `[workspace.dependencies]` `spec` / `engine` / `control` / `modbus` / `opcua` / `bacnet` `version` | Must match workspace |
| `Cargo.lock` | Regenerated by `cargo test --workspace` (do not edit by hand) |
| `CHANGELOG.md` | See steps below |
| `AGENTS.md` (this file) | “Version in this tree” |
| `README.md` roadmap | The “this tree” / current series line, if it names `v0.x` |

Do **not** bump crate version in: `docs/spec.md` `spec_version`, device YAML
`version:`, `rust-version`, edition.

### During development (every PR that ships user-visible work)

Append to `CHANGELOG.md` **`## [Unreleased]`** (Added / Changed / Fixed /
Removed). Keep that section truthful. Do not put Unreleased notes under an
old `## [x.y.z]` heading.

### Cut a release `x.y.z`

This product ships as the `simbus` binary (GitHub Release + shell installer)
and the GHCR image. Do **not** `cargo publish` the workspace crates.

Human branch rules: [CONTRIBUTING.md](CONTRIBUTING.md). Work stays on
`develop` until the cut.

Do this with CI green on `develop` (`fmt`, clippy, `cargo test --workspace --locked`,
`cargo deny check`, `simbus check` on `devices/`).

1. On **`develop`**: confirm Unreleased is complete and matches HEAD.
2. Set every row in “Version locations” to `x.y.z`.
3. In `CHANGELOG.md`: rename `## [Unreleased]` to `## [x.y.z] — YYYY-MM-DD`
   (UTC date of the tag). Insert a new empty `## [Unreleased]` **above** it.
   If the file has Keep a Changelog compare links at the bottom, point
   `[Unreleased]` at `vX.Y.Z...HEAD` and add `[x.y.z]`.
4. `cargo test --workspace --locked` (refreshes `Cargo.lock` if needed).
5. Commit on `develop` (message like `release: vX.Y.Z`). Push `develop`.
6. Open a PR **`develop` → `main`**. Merge when CI is green.
7. Annotated tag on that **`main`** commit, then push the tag:

   ```bash
   git checkout main
   git pull
   git tag -a vX.Y.Z -m "simbus vX.Y.Z"
   git push origin vX.Y.Z
   ```

   Tag shape is `v` + semver (`v0.3.0`). That commit MUST be an ancestor of
   `origin/main`. The tag triggers:

   - `.github/workflows/docker-publish.yml` → GHCR
     `ghcr.io/obsidia-systems/simbus:X.Y.Z`, `:X.Y`, `:latest`, `:sha-…`
   - `.github/workflows/release.yml` (`dist`) → GitHub Release with linux
     amd64/arm64 and macOS aarch64 archives plus `simbus-installer.sh`

8. Merge `main` back into `develop` if needed. **On `develop`**, bump
   workspace + `AGENTS.md` to the **next** version (e.g. `0.3.0` tagged →
   tree becomes `0.3.1` or `0.4.0`) so `develop` is never a lie about an
   already-published tag. Leave `main` at the shipped semver.

Do not retag. Do not tag from `develop` or a feature branch (GHCR and dist
refuse tags that are not on `main`). Do not skip hooks. Do not `--force` the
tag.

Ask the user before committing, tagging, or pushing.

