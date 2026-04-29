"""Scenario YAML loader.

Supports loading from an arbitrary file path.
Uses ruamel.yaml to preserve order and comments.
"""

from __future__ import annotations

from pathlib import Path

from ruamel.yaml import YAML

from simbus.scenarios.schema import ScenarioConfig

_yaml = YAML()
_yaml.preserve_quotes = True


def load_scenario(path: Path | str) -> ScenarioConfig:
    """Load and validate a ScenarioConfig from a YAML file path."""
    with Path(path).open(encoding="utf-8") as fh:
        data = _yaml.load(fh)
    return ScenarioConfig.model_validate(dict(data))
