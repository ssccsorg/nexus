"""Cloudflare Tunnel management (shared across engines)."""

import os
import shutil
import subprocess
import sys
import time

TUNNEL_UUID = "59c44ec1-6577-41ec-bb18-e53d95394147"


class TunnelManager:
    """Manages a cloudflared tunnel process."""

    def __init__(self, config_path: str) -> None:
        if not shutil.which("cloudflared"):
            raise RuntimeError("cloudflared not found in PATH")
        if not os.path.isfile(config_path):
            raise FileNotFoundError(f"Tunnel config not found: {config_path}")
        self.config_path = config_path
        self._process: subprocess.Popen | None = None

    def start(self) -> None:
        """Start the tunnel in the background."""
        print(f"[INFO] Starting Cloudflare Tunnel: {self.config_path}")
        self._process = subprocess.Popen(
            ["cloudflared", "tunnel", "--config", self.config_path, "run"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def stop(self) -> None:
        """Terminate the tunnel process."""
        if self._process and self._process.poll() is None:
            self._process.terminate()
            try:
                self._process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self._process.kill()

    @staticmethod
    def cleanup_stale_connections() -> None:
        """Remove stale connectors on the Cloudflare edge."""
        print("[INFO] Cleaning up stale tunnel connections...")
        subprocess.run(
            ["cloudflared", "tunnel", "cleanup", TUNNEL_UUID],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        print("[INFO] Tunnel connections cleaned.")
