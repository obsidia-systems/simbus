# ── Stage 1: build the Rust runtime ──────────────────────────────────────────
FROM rust:1-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY crates ./crates
COPY devices ./devices

RUN cargo build --release -p simbus

# ── Stage 2: runtime image ───────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN groupadd --system simbus && \
    useradd --system --gid simbus --no-create-home simbus && \
    apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/simbus /usr/local/bin/simbus
COPY --from=builder /app/devices /app/devices

ENV SIMBUS_API_PORT="8000" \
    SIMBUS_TICK_INTERVAL="1.0"

USER simbus
EXPOSE 8000 502

HEALTHCHECK \
    --interval=15s \
    --timeout=5s \
    --start-period=15s \
    --retries=3 \
    CMD curl -fsS "http://127.0.0.1:${SIMBUS_API_PORT:-8000}/healthz" || exit 1

ENTRYPOINT ["/usr/local/bin/simbus"]
