"""Scenario execution engine.

A ScenarioRunner replays a timed sequence of events against a single
SimulationEngine. It runs as an independent asyncio task so the main
tick loop is never blocked.
"""

from __future__ import annotations

import asyncio
from dataclasses import dataclass
from typing import TYPE_CHECKING

import structlog

from simbus.scenarios.schema import (
    InjectFaultStep,
    ScenarioConfig,
    SetCoilStep,
    SetRegisterStep,
    SetTickIntervalStep,
)
from simbus.simulation import behaviors
from simbus.simulation.faults import ActiveFault

if TYPE_CHECKING:
    from simbus.config.schema import DeviceConfig
    from simbus.core.store import RegisterStore
    from simbus.simulation.engine import SimulationEngine

logger = structlog.get_logger(__name__)


@dataclass
class ScenarioStatus:
    """Snapshot of the scenario runner state."""

    state: str  # idle | running | completed | stopped
    scenario_name: str | None = None
    step_index: int = 0
    total_steps: int = 0
    elapsed_s: float = 0.0


class ScenarioRunner:
    """Replays a ScenarioConfig against a running SimulationEngine."""

    def __init__(
        self,
        engine: SimulationEngine,
        store: RegisterStore,
        config: DeviceConfig,
    ) -> None:
        self._engine = engine
        self._store = store
        self._config = config
        self._task: asyncio.Task[None] | None = None
        self._status = ScenarioStatus(state="idle")

    # ------------------------------------------------------------------
    # Public API
    # ------------------------------------------------------------------

    @property
    def status(self) -> ScenarioStatus:
        """Current runner status."""
        return self._status

    def run(self, scenario: ScenarioConfig) -> None:
        """Start replaying *scenario* in a background asyncio task.

        If a scenario is already running it is cancelled first.
        """
        self.stop()
        self._task = asyncio.create_task(
            self._run_loop(scenario),
            name=f"scenario-{scenario.name}",
        )
        logger.info(
            "scenario started",
            source="scenario",
            scenario=scenario.name,
            steps=len(scenario.steps),
        )

    def stop(self) -> None:
        """Cancel any active scenario."""
        if self._task is not None and not self._task.done():
            self._task.cancel()
            self._status = ScenarioStatus(state="stopped", scenario_name=self._status.scenario_name)
            logger.info("scenario stopped", source="scenario")
        self._task = None

    # ------------------------------------------------------------------
    # Internal loop
    # ------------------------------------------------------------------

    async def _run_loop(self, scenario: ScenarioConfig) -> None:
        steps = sorted(scenario.steps, key=lambda s: s.at)
        total = len(steps)
        self._status = ScenarioStatus(
            state="running",
            scenario_name=scenario.name,
            step_index=0,
            total_steps=total,
            elapsed_s=0.0,
        )

        started = asyncio.get_event_loop().time()
        try:
            for idx, step in enumerate(steps):
                now = asyncio.get_event_loop().time()
                target = started + step.at
                wait = target - now
                if wait > 0:
                    await asyncio.sleep(wait)

                self._status.step_index = idx + 1
                self._status.elapsed_s = asyncio.get_event_loop().time() - started
                self._execute(step)

            self._status.state = "completed"
            logger.info(
                "scenario completed",
                source="scenario",
                scenario=scenario.name,
                steps=total,
            )
        except asyncio.CancelledError:
            self._status.state = "stopped"
            logger.info("scenario cancelled", source="scenario", scenario=scenario.name)
            raise

    # ------------------------------------------------------------------
    # Step dispatch
    # ------------------------------------------------------------------

    def _execute(self, step: SetRegisterStep | InjectFaultStep | SetCoilStep | SetTickIntervalStep) -> None:
        match step:
            case SetRegisterStep():
                self._exec_set_register(step)
            case InjectFaultStep():
                self._exec_inject_fault(step)
            case SetCoilStep():
                self._exec_set_coil(step)
            case SetTickIntervalStep():
                self._exec_set_tick_interval(step)

    def _exec_set_register(self, step: SetRegisterStep) -> None:
        reg = self._find_register(step.register_name, step.register_type)
        if reg is None:
            logger.warning(
                "scenario step skipped: unknown register",
                source="scenario",
                register=step.register_name,
                register_type=step.register_type,
            )
            return

        raw = behaviors.scale_to_raw(step.value, reg.scale)
        if step.register_type == "input":
            self._store.set_input(reg.address, raw)
        else:
            self._store.set_holding(reg.address, raw)
        self._engine.update_base(reg.address, raw, source="scenario")
        logger.info(
            "scenario set_register",
            source="scenario",
            register=step.register_name,
            register_type=step.register_type,
            real_value=step.value,
            raw_value=raw,
        )

    def _exec_inject_fault(self, step: InjectFaultStep) -> None:
        self._engine.inject_fault(
            ActiveFault(
                fault_type=step.fault_type,
                register_name=step.register_name,
                value=step.value,
                duration_s=step.duration_s,
                remaining_s=step.duration_s,
            )
        )
        logger.info(
            "scenario inject_fault",
            source="scenario",
            fault_type=step.fault_type.value,
            register_name=step.register_name,
            duration_s=step.duration_s,
        )

    def _exec_set_coil(self, step: SetCoilStep) -> None:
        coil = self._find_coil(step.coil)
        if coil is None:
            logger.warning(
                "scenario step skipped: unknown coil/discrete",
                source="scenario",
                coil=step.coil,
            )
            return

        if coil in self._config.registers.coils:
            self._store.set_coil(coil.address, step.value)
        else:
            self._store.set_discrete(coil.address, step.value)
        logger.info(
            "scenario set_coil",
            source="scenario",
            coil=step.coil,
            value=step.value,
        )

    def _exec_set_tick_interval(self, step: SetTickIntervalStep) -> None:
        self._engine.tick_interval = step.tick_interval
        logger.info(
            "scenario set_tick_interval",
            source="scenario",
            tick_interval=step.tick_interval,
        )

    # ------------------------------------------------------------------
    # Helpers
    # ------------------------------------------------------------------

    def _find_register(self, name: str, reg_type: str):  # type: ignore[no-untyped-def]
        regs = self._config.registers.input if reg_type == "input" else self._config.registers.holding
        for r in regs:
            if r.name == name:
                return r
        return None

    def _find_coil(self, name: str):  # type: ignore[no-untyped-def]
        for c in self._config.registers.coils + self._config.registers.discrete:
            if c.name == name:
                return c
        return None
