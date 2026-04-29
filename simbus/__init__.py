"""Simbus — Industrial Field Device Simulator."""

from importlib.metadata import version as get_version

try:
    __version__ = get_version("simbus")
except Exception:  # pragma: no cover
    __version__ = "0.1.0"
