"""Logging configuration for simbus.

Sets up a concise console renderer for local development while keeping
structured key/value events from structlog.
"""

from __future__ import annotations

import logging

import structlog

_configured = False


def configure_logging() -> None:
    """Configure stdlib logging + structlog once per process."""
    global _configured
    if _configured:
        return

    logging.basicConfig(level=logging.INFO, format="%(message)s")
    logging.getLogger("pymodbus").setLevel(logging.CRITICAL)
    logging.getLogger("uvicorn.access").setLevel(logging.CRITICAL)

    structlog.configure(
        processors=[
            structlog.stdlib.add_log_level,
            structlog.processors.TimeStamper(fmt="iso"),
            structlog.processors.StackInfoRenderer(),
            structlog.dev.ConsoleRenderer(),
        ],
        wrapper_class=structlog.stdlib.BoundLogger,
        logger_factory=structlog.stdlib.LoggerFactory(),
        cache_logger_on_first_use=True,
    )

    _configured = True
