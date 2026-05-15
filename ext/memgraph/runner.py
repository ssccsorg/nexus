"""Memgraph engine — in-memory graph database with HTTP-to-bolt proxy."""

import os
import shutil
import subprocess
import time
from urllib import request

from _runner.base import AbstractEngine, EngineInfo


class MemgraphEngine(AbstractEngine):
    @property
    def name(self) -> str:
        return "memgraph"

    def __init__(self, graph_dir: str) -> None:
        self.graph_dir = graph_dir
        self.proxy_dir = os.path.join(graph_dir, "memgraph")
        self.proxy_image = "memgraph-proxy:latest"
        self.proxy_container = "memgraph-proxy"
        self.mg_container = "memgraph-nexus"
        self.proxy_port = 7689
        self.mg_port = 7688
        self.lab_port = 3001

    @property
    def tunnel_config(self) -> str:
        return os.path.join(self.proxy_dir, "tunnel-config.yml")

    def check(self) -> bool:
        return shutil.which("docker") is not None

    def start(self, refresh: bool = False) -> None:
        # Start or ensure Memgraph is running
        mg_running = subprocess.run(
            ["docker", "inspect", "-f", "{{.State.Running}}", self.mg_container],
            capture_output=True, text=True,
        )
        if mg_running.stdout.strip() != "true":
            print("[INFO] Starting Memgraph...")
            subprocess.run(
                ["docker", "rm", "-f", self.mg_container],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            subprocess.run([
                "docker", "run", "-d",
                "--name", self.mg_container,
                "--restart", "unless-stopped",
                "-p", f"{self.mg_port}:7687",
                "-p", f"{self.lab_port}:3000",
                "-p", "7444:7444",
                "memgraph/memgraph-platform",
            ], check=True)
            print("[INFO] Memgraph started.")

        # Build and start proxy
        subprocess.run(
            ["docker", "rm", "-f", self.proxy_container],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )

        # Build proxy image if needed
        subprocess.run(
            ["docker", "build", "-t", self.proxy_image, "-f", "Dockerfile", "."],
            cwd=self.proxy_dir,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )

        subprocess.run([
            "docker", "run", "-d",
            "--name", self.proxy_container,
            "--restart", "unless-stopped",
            "-p", f"{self.proxy_port}:7689",
            "-e", "MEMGRAPH_HOST=host.docker.internal",
            "-e", f"MEMGRAPH_PORT={self.mg_port}",
            "-e", "PROXY_PORT=7689",
            self.proxy_image,
        ], check=True)
        print("[INFO] Memgraph proxy started.")

    def stop(self) -> None:
        subprocess.run(
            ["docker", "rm", "-f", self.proxy_container],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )

    def health_check(self, timeout_sec: int) -> bool:
        print("[INFO] Waiting for Memgraph proxy...")
        deadline = time.time() + timeout_sec
        while time.time() < deadline:
            try:
                req = request.Request(f"http://127.0.0.1:{self.proxy_port}/health")
                with request.urlopen(req, timeout=3) as resp:
                    if resp.status == 200:
                        print("[INFO] Memgraph proxy healthy.")
                        return True
            except Exception:
                pass
            time.sleep(2)
        print(f"[ERROR] Memgraph proxy not healthy within {timeout_sec}s.")
        return False

    def info(self) -> EngineInfo:
        return EngineInfo(
            name="Memgraph",
            entries={
                "Bolt": f"localhost:{self.mg_port}",
                "Proxy": f"http://localhost:{self.proxy_port}",
                "Lab UI": f"http://localhost:{self.lab_port}",
            },
        )
