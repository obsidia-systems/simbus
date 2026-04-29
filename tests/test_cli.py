"""Unit tests for the CLI entrypoint."""

from __future__ import annotations

from unittest.mock import patch

from typer.testing import CliRunner

from simbus.cli import app

runner = CliRunner()


class TestCLI:
    def test_no_args_shows_help(self) -> None:
        result = runner.invoke(app, [])
        assert result.exit_code != 0
        assert "Usage:" in result.output or "Error" in result.output

    def test_start_requires_type_or_file(self) -> None:
        result = runner.invoke(app, ["start"])
        assert result.exit_code != 0

    @patch("simbus.cli.uvicorn.run")
    def test_start_with_type_invokes_uvicorn(self, mock_uvicorn) -> None:
        result = runner.invoke(app, ["--type", "generic-tnh-sensor", "--api-port", "8000"])
        assert result.exit_code == 0
        mock_uvicorn.assert_called_once()
        call_kwargs = mock_uvicorn.call_args[1]
        assert call_kwargs["host"] == "0.0.0.0"
        assert call_kwargs["port"] == 8000
        assert call_kwargs["log_level"] == "warning"
        assert call_kwargs["access_log"] is False
