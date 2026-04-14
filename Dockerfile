# ── Stage 1: dependency resolver ────────────────────────────────────────────
# uv is used only here; the runtime image stays uv-free.
FROM python:3.14-slim AS builder

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    UV_LINK_MODE=copy

COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/

WORKDIR /app

# Layer A: resolve & install dependencies (cached unless lock file changes)
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev --no-install-project

# Layer B: copy source and install the project package itself
# README.md is required by hatchling to build the package metadata.
COPY README.md ./
COPY simbus/ ./simbus/
RUN uv sync --frozen --no-dev


# ── Stage 2: runtime image ───────────────────────────────────────────────────
FROM python:3.14-slim AS runtime

ENV PYTHONDONTWRITEBYTECODE=1 \
    PYTHONUNBUFFERED=1 \
    PATH="/app/.venv/bin:$PATH" \
    # Defaults — override via env vars or docker-compose
    SIMBUS_DEVICE_TYPE="generic-tnh-sensor" \
    SIMBUS_API_PORT="8000" \
    SIMBUS_TICK_INTERVAL="1.0"

WORKDIR /app

# Non-root user — reduces attack surface
RUN groupadd --system simbus && \
    useradd --system --gid simbus --no-create-home simbus

# Copy only runtime artifacts from the builder
COPY --from=builder --chown=simbus:simbus /app/.venv /app/.venv
COPY --from=builder --chown=simbus:simbus /app/simbus /app/simbus

USER simbus

# REST API
EXPOSE 8000
# Modbus TCP — clients connect here
EXPOSE 502

HEALTHCHECK \
    --interval=15s \
    --timeout=5s \
    --start-period=15s \
    --retries=3 \
    CMD python -c \
        "import urllib.request, os; \
         urllib.request.urlopen( \
             'http://localhost:' + os.getenv('SIMBUS_API_PORT','8000') + '/status' \
         )"

# Start through the simbus CLI so logging and runtime behavior stay consistent
# across local runs, tests, and containers.
CMD ["/bin/sh", "-c", "\
PORT_ARG=\"\"; \
if [ -n \"${SIMBUS_MODBUS_PORT:-}\" ]; then PORT_ARG=\"--port ${SIMBUS_MODBUS_PORT}\"; fi; \
if [ -n \"${SIMBUS_YAML_PATH:-}\" ]; then \
  exec simbus \
    --file \"${SIMBUS_YAML_PATH}\" \
    --api-port \"${SIMBUS_API_PORT:-8000}\" \
    --host \"${SIMBUS_API_HOST:-0.0.0.0}\" \
    --tick \"${SIMBUS_TICK_INTERVAL:-1.0}\" \
    ${PORT_ARG}; \
else \
  exec simbus \
    --type \"${SIMBUS_DEVICE_TYPE:-generic-tnh-sensor}\" \
    --api-port \"${SIMBUS_API_PORT:-8000}\" \
    --host \"${SIMBUS_API_HOST:-0.0.0.0}\" \
    --tick \"${SIMBUS_TICK_INTERVAL:-1.0}\" \
    ${PORT_ARG}; \
fi"]
