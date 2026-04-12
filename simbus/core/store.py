"""In-memory Modbus register state for a single virtual device.

Design note — no asyncio.Lock:
  All reads and writes happen within a single asyncio event loop.
  asyncio is cooperative: a task only yields at an `await` point.
  Since get/set methods here are synchronous (no awaits), they execute
  atomically relative to other tasks. No lock is needed.

  The Modbus server (pymodbus) calls getValues/setValues synchronously
  from within the same event loop, so there is no threading concern.
"""

from __future__ import annotations

from dataclasses import dataclass

from simbus.config.schema import RegisterMapConfig


@dataclass(slots=True)
class RegisterSnapshot:
    """Immutable point-in-time copy of all register values."""

    holding: dict[int, int]
    input: dict[int, int]
    coils: dict[int, bool]
    discrete: dict[int, bool]


class RegisterStore:
    """In-memory Modbus register bank for a single device."""

    def __init__(self) -> None:
        self._holding: dict[int, int] = {}
        self._input: dict[int, int] = {}
        self._coils: dict[int, bool] = {}
        self._discrete: dict[int, bool] = {}

    def initialize(self, register_map: RegisterMapConfig) -> None:
        """Seed registers with default values from the device config."""
        for reg in register_map.holding:
            self._holding[reg.address] = _scale(reg.default, reg.scale)
        for reg in register_map.input:
            self._input[reg.address] = _scale(reg.default, reg.scale)
        for coil in register_map.coils:
            self._coils[coil.address] = coil.default
        for coil in register_map.discrete:
            self._discrete[coil.address] = coil.default

    # --- Holding registers (read/write) ---

    def get_holding(self, address: int) -> int:
        """Return the value of a holding register, or 0 if not set."""
        return self._holding.get(address, 0)

    def set_holding(self, address: int, value: int) -> None:
        """Set the value of a holding register, masking to 16 bits."""
        self._holding[address] = value & 0xFFFF

    # --- Input registers (read-only by Modbus clients) ---

    def get_input(self, address: int) -> int:
        """Return the value of an input register, or 0 if not set."""
        return self._input.get(address, 0)

    def set_input(self, address: int, value: int) -> None:
        """Set the value of an input register, masking to 16 bits."""
        self._input[address] = value & 0xFFFF

    # --- Coils (read/write) ---

    def get_coil(self, address: int) -> bool:
        """Return the value of a coil, or False if not set."""
        return self._coils.get(address, False)

    def set_coil(self, address: int, value: bool) -> None:
        """Set the value of a coil."""
        self._coils[address] = value

    # --- Discrete inputs (read-only by Modbus clients) ---

    def get_discrete(self, address: int) -> bool:
        """Return the value of a discrete input, or False if not set."""
        return self._discrete.get(address, False)

    def set_discrete(self, address: int, value: bool) -> None:
        """Set the value of a discrete input."""
        self._discrete[address] = value

    def snapshot(self) -> RegisterSnapshot:
        """Return an immutable copy of the current register state."""
        return RegisterSnapshot(
            holding=dict(self._holding),
            input=dict(self._input),
            coils=dict(self._coils),
            discrete=dict(self._discrete),
        )

    # --- Raw dict access for pymodbus DataBlock integration ---

    @property
    def holding_raw(self) -> dict[int, int]:
        """Return the raw dict of holding register values."""
        return self._holding

    @property
    def input_raw(self) -> dict[int, int]:
        """Return the raw dict of input register values."""
        return self._input

    @property
    def coils_raw(self) -> dict[int, bool]:
        """Return the raw dict of coil values."""
        return self._coils

    @property
    def discrete_raw(self) -> dict[int, bool]:
        """Return the raw dict of discrete input values."""
        return self._discrete


def _scale(value: float, scale: int) -> int:
    """Convert a real-world float to a scaled uint16 integer.

    Example: 22.5 °C with scale=10 → raw register value 225.
    """
    return int(round(value * scale)) & 0xFFFF
