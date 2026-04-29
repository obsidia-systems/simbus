"""Scenario control API endpoints.

GET    /scenarios            → list available scenario files
POST   /scenarios/{name}/run → start replaying a scenario
GET    /scenarios/active     → current runner status
POST   /scenarios/stop       → cancel active scenario
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

import structlog
from fastapi import APIRouter, HTTPException, Request, status

from simbus.scenarios.loader import load_scenario

logger = structlog.get_logger(__name__)
router = APIRouter()


def _discover_scenarios() -> dict[str, Path]:
    """Discover scenario YAML files in ./scenarios/ and built-ins."""
    found: dict[str, Path] = {}
    for folder in [Path("scenarios"), Path(__file__).parent.parent.parent / "scenarios" / "builtin"]:
        if folder.exists() and folder.is_dir():
            for f in sorted(folder.glob("*.yaml")):
                found.setdefault(f.stem, f)
    return found


@router.get("", summary="List available scenarios")
async def list_scenarios() -> list[dict[str, str]]:
    """Return names and descriptions of all discoverable scenario YAML files."""
    scenarios = _discover_scenarios()
    result = []
    for name, path in sorted(scenarios.items()):
        try:
            cfg = load_scenario(path)
            result.append({"name": name, "description": cfg.description})
        except Exception:
            result.append({"name": name, "description": ""})
    return result


@router.post(
    "/{name}/run",
    status_code=status.HTTP_202_ACCEPTED,
    summary="Start a scenario",
)
async def run_scenario(name: str, request: Request) -> dict[str, Any]:
    """Load and start replaying the named scenario."""
    scenarios = _discover_scenarios()
    if name not in scenarios:
        raise HTTPException(
            status_code=status.HTTP_404_NOT_FOUND,
            detail=f"Scenario '{name}' not found",
        )

    cfg = load_scenario(scenarios[name])
    runner = request.app.state.scenario_runner
    runner.run(cfg)
    logger.info("scenario run requested", source="api", scenario=name)
    return {"status": "started", "scenario": name, "steps": len(cfg.steps)}


@router.get("/active", summary="Get active scenario status")
async def active_scenario(request: Request) -> dict[str, Any]:
    """Return the current scenario runner state."""
    runner = request.app.state.scenario_runner
    s = runner.status
    return {
        "state": s.state,
        "scenario_name": s.scenario_name,
        "step_index": s.step_index,
        "total_steps": s.total_steps,
        "elapsed_s": round(s.elapsed_s, 3),
    }


@router.post("/stop", status_code=status.HTTP_204_NO_CONTENT, summary="Stop active scenario")
async def stop_scenario(request: Request) -> None:
    """Cancel any running scenario immediately."""
    runner = request.app.state.scenario_runner
    runner.stop()
    logger.info("scenario stop requested", source="api")
