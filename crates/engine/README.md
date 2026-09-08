# engine

In-memory device: register bank, tick loop, faults, and scenario steps. No network I/O.

Each register gets its own phase offset and (for noise/drift) a jittered operating point, so a fleet of containers does not replay the same curve. `--seed` is mixed with `name`, `type`, and `identity`. Drift is `rate × dt` (engineering units per simulation second). `reset` restores boot `RegState` and the RNG stream so a seeded device replays the same trace.

## Tests

```bash
cargo test -p engine
```

- Unit: behaviors and encode in `src/behaviors.rs` / `src/encode.rs`
- Integration (`tests/engine.rs`): T&H defaults, faults (spike, freeze latch, dropout, alarm), triggers, `rate × dt`, reset replay, seed / identity mix, FC16-style `write_words` (adjacent uint16, atomic holes, splice into float32, read range 02)
