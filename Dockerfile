# ── Stage 1: build a static musl binary ──────────────────────────────────────
FROM rust:1-alpine AS builder

RUN apk add --no-cache build-base musl-dev curl zip unzip

# utoipa-swagger-ui embeds the Swagger UI dist verbatim in .rodata. The upstream
# zip carries source maps and duplicate ES bundles that the served page never
# requests — roughly 10 MB of dead weight in the binary. Repack without them.
# Keep this version in step with SWAGGER_UI_DOWNLOAD_URL_DEFAULT in the crate's
# build.rs; a mismatch silently ships a different Swagger UI than a local build.
ARG SWAGGER_UI_VERSION=5.17.14
RUN mkdir -p /opt/swagger && cd /opt/swagger && \
    curl -sSLo swagger.zip "https://github.com/swagger-api/swagger-ui/archive/refs/tags/v${SWAGGER_UI_VERSION}.zip" && \
    unzip -q swagger.zip && rm swagger.zip && \
    find "swagger-ui-${SWAGGER_UI_VERSION}/dist" \
        \( -name '*.map' -o -name '*es-bundle*' \) -delete && \
    zip -qr /opt/swagger-ui-slim.zip "swagger-ui-${SWAGGER_UI_VERSION}"
ENV SWAGGER_UI_DOWNLOAD_URL="file:///opt/swagger-ui-slim.zip"

WORKDIR /app
COPY Cargo.toml Cargo.lock rust-toolchain.toml rustfmt.toml ./
COPY crates ./crates
COPY devices ./devices

RUN cargo build --release -p simbus

# ── Stage 2: runtime image ───────────────────────────────────────────────────
# musl targets link statically, so the runtime needs no libc. distroless/static
# supplies only CA certificates, tzdata and the nonroot (65532) passwd entry.
FROM gcr.io/distroless/static-debian12:nonroot AS runtime

WORKDIR /app
COPY --from=builder /app/target/release/simbus /usr/local/bin/simbus
COPY --from=builder /app/devices /app/devices

ENV SIMBUS_API_PORT="8000" \
    SIMBUS_TICK_INTERVAL="1.0"

EXPOSE 8000 502

# No shell in this image: `simbus ctl` is the health probe. It reads
# SIMBUS_API_PORT itself, so an API port override still probes the right port.
HEALTHCHECK \
    --interval=15s \
    --timeout=5s \
    --start-period=15s \
    --retries=3 \
    CMD ["/usr/local/bin/simbus", "ctl", "healthz"]

ENTRYPOINT ["/usr/local/bin/simbus"]
