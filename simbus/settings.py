"""Runtime settings for a single simbus device instance.

Read from environment variables (prefix: SIMBUS_) or a .env file.
The CLI sets these before handing off to uvicorn.

Example .env:
    SIMBUS_DEVICE_TYPE=generic-tnh-sensor
    SIMBUS_MODBUS_PORT=5020
    SIMBUS_TICK_INTERVAL=1.0
"""

from __future__ import annotations

from pydantic import Field
from pydantic_settings import BaseSettings, SettingsConfigDict


class DeviceSettings(BaseSettings):
    model_config = SettingsConfigDict(
        env_prefix="SIMBUS_",
        env_file=".env",
        env_file_encoding="utf-8",
        extra="ignore",
    )

    # Device source — set one via env var or CLI flag.
    # Defaults to generic-tnh-sensor so `fastapi dev` and bare uvicorn work
    # out of the box for development without extra configuration.
    device_type: str | None = Field(
        default="generic-tnh-sensor",
        description="Built-in device type (e.g. 'generic-tnh-sensor')",
    )
    yaml_path: str | None = Field(
        default=None,
        description="Path to a custom device YAML file. Takes precedence over device_type.",
    )

    # Network
    modbus_port: int | None = Field(default=None, ge=1, le=65535)
    api_host: str = Field(default="0.0.0.0")  # noqa: S104
    api_port: int = Field(default=8000, ge=1024, le=65535)

    # Simulation
    tick_interval: float = Field(
        default=1.0, gt=0, description="Tick interval in seconds")
    tick_health_log_interval: float = Field(
        default=60.0,
        gt=0,
        description="How often to emit simulation tick health logs, in seconds.",
    )
    seed: int | None = Field(
        default=None, description="RNG seed for reproducible behavior")

    # Optional name override (takes precedence over the YAML name field)
    device_name: str | None = Field(default=None)

    # CORS — set to specific origins in production, e.g. "http://localhost:5173,https://my-gui.com"
    cors_origins: list[str] = Field(
        default=["*"],
        description="Allowed CORS origins. Use ['*'] for development.",
    )
