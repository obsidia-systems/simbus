"""YAML device config loader.

Supports loading from an arbitrary file path or by built-in device type name.
Uses ruamel.yaml to preserve order and comments (important for round-trip
editing and community contributions).
"""

from __future__ import annotations

from importlib import resources
from pathlib import Path
from typing import Final

from ruamel.yaml import YAML

from simbus.config.schema import DeviceConfig

_yaml = YAML()
_yaml.preserve_quotes = True

BUILTIN_DEVICES: Final[frozenset[str]] = frozenset(
    {
        "generic-tnh-sensor",
        "generic-ups",
        "generic-pdu",
        "generic-crac",
        "generic-power-meter",
        "generic-leak-sensor",
        "generic-door-contact",
    }
)


def load_from_file(path: Path | str) -> DeviceConfig:
    """Load and validate a DeviceConfig from a YAML file path."""
    with Path(path).open(encoding="utf-8") as fh:
        data = _yaml.load(fh)
    return DeviceConfig.model_validate(dict(data))


def load_builtin(device_type: str) -> DeviceConfig:
    """Load a built-in device definition embedded in the package.

    Args:
        device_type: One of the names in BUILTIN_DEVICES.

    Raises:
        ValueError: If the device type is not known.
        FileNotFoundError: If the YAML file is missing from the package.
    """
    if device_type not in BUILTIN_DEVICES:
        available = ", ".join(sorted(BUILTIN_DEVICES))
        raise ValueError(
            f"Unknown built-in device type '{device_type}'. "
            f"Available: {available}"
        )

    pkg = resources.files("simbus.builtin")
    ref = pkg / f"{device_type}.yaml"
    with resources.as_file(ref) as path:
        return load_from_file(path)
