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
    SIMBUS_MODBUS_PORT="5020" \
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
EXPOSE 5020

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

# All config is read from SIMBUS_* env vars at startup.
# The module-level `app = create_app()` in main.py picks up DeviceSettings().
# exec replaces the shell so uvicorn receives SIGTERM directly (clean shutdown).
CMD ["/bin/sh", "-c", "exec uvicorn simbus.api.main:app --host 0.0.0.0 --port \"${SIMBUS_API_PORT:-8000}\" --log-level info"]
