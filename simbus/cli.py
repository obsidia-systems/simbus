"""CLI entrypoint — `simbus` command.

Each invocation starts a single device instance:
  simbus --type generic-tnh-sensor --port 502 --name hot-aisle-01
  simbus --file ./my-ups.yaml --port 512

The CLI:
  1. Builds a DeviceSettings object from the given flags.
  2. Creates the FastAPI app with those settings pre-injected.
  3. Hands off to uvicorn.

This way the lifespan receives settings directly — no env var round-trip.
"""

from __future__ import annotations

from typing import Optional

import typer
import uvicorn

app = typer.Typer(
    name="simbus",
    help="Industrial Field Device Simulator — Modbus TCP",
    no_args_is_help=True,
)


@app.command()
def start(
    device_type: Optional[str] = typer.Option(  # noqa: UP007
        None,
        "--type",
        help="Built-in device type (e.g. generic-tnh-sensor)",
    ),
    file: Optional[str] = typer.Option(  # noqa: UP007
        None,
        "--file",
        "-f",
        help="Path to a custom device YAML file",
    ),
    port: Optional[int] = typer.Option(None, "--port", "-p", help="Modbus TCP port"),  # noqa: UP007
    name: Optional[str] = typer.Option(None, "--name", "-n", help="Override device name"),  # noqa: UP007
    api_port: int = typer.Option(8000, "--api-port", help="REST API port"),
    host: str = typer.Option("0.0.0.0", "--host", help="Bind address"),  # noqa: S104
    tick: float = typer.Option(
        1.0, "--tick", help="Simulation tick interval (seconds)"),
    seed: Optional[int] = typer.Option(None, "--seed", help="RNG seed for reproducibility"),  # noqa: UP007
) -> None:
    """Start a virtual device with its Modbus server and REST API."""
    if device_type is None and file is None:
        typer.echo("Error: provide --type or --file", err=True)
        raise typer.Exit(code=1)

    from simbus.api.main import create_app
    from simbus.settings import DeviceSettings

    settings = DeviceSettings(
        device_type=device_type,
        yaml_path=file,
        modbus_port=port,
        api_host=host,
        api_port=api_port,
        tick_interval=tick,
        seed=seed,
        device_name=name,
    )

    fastapi_app = create_app(settings=settings)

    modbus_port_display = port if port is not None else "yaml/default"
    typer.echo(
        f"Starting '{name or device_type or file}' — Modbus :{modbus_port_display}  API :{api_port}")
    uvicorn.run(
        fastapi_app,
        host=host,
        port=api_port,
        log_level="warning",
        access_log=False,
    )
