"""Stdlib-only Python client for the local L5M stdio server."""

from __future__ import annotations

import json
import shlex
import subprocess
from pathlib import Path
from typing import Any, Iterable


class L5MClient:
    """Keep L5M retrieval in Rust and query it from Python over stdio."""

    def __init__(
        self,
        segment: str | Path,
        binary: str | Iterable[str] = "cargo run -q -p l5m-cli --",
        cwd: str | Path | None = None,
    ) -> None:
        self.segment = Path(segment)
        self.binary = binary
        self.cwd = Path(cwd) if cwd is not None else None
        self.process: subprocess.Popen[str] | None = None

    def __enter__(self) -> "L5MClient":
        self.start()
        return self

    def __exit__(self, exc_type: object, exc: object, tb: object) -> None:
        self.close()

    def start(self) -> None:
        if self.process is not None:
            return
        if not self.segment.exists():
            raise FileNotFoundError(f"segment does not exist: {self.segment}")

        command = self._binary_args() + [
            "serve-stdio",
            "--segment",
            str(self.segment),
        ]
        self.process = subprocess.Popen(
            command,
            cwd=str(self.cwd) if self.cwd is not None else None,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )

    def query(self, request: dict[str, Any]) -> dict[str, Any]:
        self.start()
        assert self.process is not None
        if self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError("l5m stdio pipes are unavailable")

        self.process.stdin.write(json.dumps(request) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(stderr.strip() or "l5m process exited without a response")
        return json.loads(line)

    def close(self) -> None:
        if self.process is None:
            return
        if self.process.stdin is not None:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=5)
        finally:
            if self.process.stdout is not None:
                self.process.stdout.close()
            if self.process.stderr is not None:
                self.process.stderr.close()
            self.process = None

    def _binary_args(self) -> list[str]:
        if isinstance(self.binary, str):
            return shlex.split(self.binary)
        return [str(part) for part in self.binary]


__all__ = ["L5MClient"]
