FROM python:3.14-slim AS builder

ENV PYTHONDONTWRITEBYTECODE=1 \
	PYTHONUNBUFFERED=1 \
	UV_LINK_MODE=copy

# Instala uv desde Docker Hub (astral/uv) solo en la etapa de build.
COPY --from=astral/uv:latest /uv /uvx /bin/

WORKDIR /app

# Copiamos dependencias primero para maximizar cache.
COPY pyproject.toml uv.lock ./

# Resuelve e instala dependencias en .venv de forma reproducible.
RUN uv sync --frozen --no-dev --no-install-project


FROM python:3.14-slim AS runtime

ENV PYTHONDONTWRITEBYTECODE=1 \
	PYTHONUNBUFFERED=1 \
	PATH="/app/.venv/bin:$PATH" \
	MODBUS_PORT=1502

WORKDIR /app

# Usuario no-root para reducir superficie de ataque.
RUN groupadd --system app && useradd --system --gid app --create-home app

# Copia solo artefactos de runtime (sin herramientas de build).
COPY --from=builder /app/.venv /app/.venv
COPY --chown=app:app main.py ./

USER app

EXPOSE 1502

CMD ["python", "main.py"]