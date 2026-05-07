"""Graphiti engine — temporal knowledge graph (Docker)."""

import os
import shutil
import subprocess
import time
from urllib import request

from runners.base import AbstractEngine, EngineInfo
from runners.checks import detect_embedding_dimension


class GraphitiEngine(AbstractEngine):
    @property
    def name(self) -> str:
        return "graphiti"

    def __init__(self, rag_dir: str) -> None:
        self.rag_dir = rag_dir
        self.image = os.environ.get("GRAPHITI_IMAGE", "graphiti-nexus")
        self.container = os.environ.get("GRAPHITI_CONTAINER", "graphiti-nexus")
        self.engine_dir = os.path.join(rag_dir, "graphiti")
        self.dockerfile = os.path.join(self.engine_dir, "Dockerfile")
        self.port = int(os.environ.get("GRAPHITI_PORT", "8000"))
        self.host = os.environ.get("GRAPHITI_HOST", "0.0.0.0")

    @property
    def tunnel_config(self) -> str:
        return os.path.join(self.engine_dir, "tunnel-config.yml")

    def check(self) -> bool:
        if not shutil.which("docker"):
            print("[ERROR] Docker is required for Graphiti.")
            return False
        if not os.path.isfile(self.dockerfile):
            print(f"[ERROR] Dockerfile not found: {self.dockerfile}")
            return False

        result = subprocess.run(
            ["docker", "image", "inspect", self.image],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if result.returncode != 0:
            print(f"[INFO] Building Graphiti Docker image {self.image}...")
            subprocess.run(
                [
                    "docker",
                    "build",
                    "-t",
                    self.image,
                    "-f",
                    self.dockerfile,
                    self.engine_dir,
                ],
                check=True,
            )
        return True

    def start(self, refresh: bool = False) -> None:
        from runners.defaults import (
            LMSTUDIO_URL as lmstudio_url,
            LLM_MODEL as llm_model,
            EMBEDDING_MODEL as embedding_model,
        )

        dim = detect_embedding_dimension(
            lmstudio_url,
            embedding_model,
            env_override=os.environ.get("EMBEDDING_DIMENSION"),
        )

        os.makedirs(os.path.join(self.rag_dir, "logs"), exist_ok=True)

        print(f"[INFO] LLM    : {llm_model}")
        print(f"[INFO] Embed  : {embedding_model} ({dim}d)")
        print(f"[INFO] Neo4j  : neo4j:7687")

        # Remove existing container
        subprocess.run(
            ["docker", "rm", "-f", self.container],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        if refresh:
            print("[INFO] Refresh requested — removing Neo4j data volume...")
            subprocess.run(
                ["docker", "volume", "rm", "-f", "graphiti-neo4j-data"],
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )

        # Ensure Neo4j is running
        neo4j_running = subprocess.run(
            ["docker", "inspect", "-f", "{{.State.Running}}", "graphiti-neo4j"],
            capture_output=True,
            text=True,
        )
        if neo4j_running.stdout.strip() != "true":
            print("[INFO] Starting Neo4j for Graphiti...")
            subprocess.run(
                [
                    "docker", "run", "-d",
                    "--name", "graphiti-neo4j",
                    "--restart", "unless-stopped",
                    "-p", "7474:7474",
                    "-p", "7687:7687",
                    "-v", "graphiti-neo4j-data:/data",
                    "-e", "NEO4J_AUTH=neo4j/graphiti",
                    "neo4j:5",
                ],
                check=True,
            )
            # Give Neo4j time to start
            print("[INFO] Waiting for Neo4j to be ready...")
            for _ in range(30):
                try:
                    r = subprocess.run(
                        [
                            "docker", "exec", "graphiti-neo4j",
                            "cypher-shell", "-u", "neo4j", "-p", "graphiti",
                            "RETURN 1",
                        ],
                        capture_output=True,
                        timeout=5,
                    )
                    if r.returncode == 0:
                        break
                except Exception:
                    pass
                time.sleep(2)
            else:
                print("[WARN] Neo4j may not be fully ready")

        cmd = [
            "docker",
            "run",
            "-d",
            "--name",
            self.container,
            "--restart",
            "unless-stopped",
            "--add-host",
            "host.docker.internal:host-gateway",
            "--link", "graphiti-neo4j:neo4j",
            "-p",
            f"{self.port}:8000",
            "-e",
            f"HOST={self.host}",
            "-e",
            "PORT=8000",
            "-e",
            f"OPENAI_BASE_URL={lmstudio_url}/v1",
            "-e",
            f"LLM_MODEL={llm_model}",
            "-e",
            f"EMBEDDING_MODEL={embedding_model}",
            "-e",
            f"EMBEDDING_DIM={dim}",
            "-e",
            "NEO4J_URI=bolt://neo4j:7687",
            "-e",
            "NEO4J_USER=neo4j",
            "-e",
            "NEO4J_PASSWORD=graphiti",
            "-e",
            "WORKSPACE=default",
            self.image,
        ]
        subprocess.run(cmd, check=True)

    def stop(self) -> None:
        subprocess.run(
            ["docker", "rm", "-f", self.container],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def health_check(self, timeout_sec: int) -> bool:
        print("[INFO] Waiting for Graphiti health check...")
        deadline = time.time() + timeout_sec
        while time.time() < deadline:
            try:
                req = request.Request(f"http://127.0.0.1:{self.port}/health")
                with request.urlopen(req, timeout=3) as resp:
                    body = resp.read().decode()
                    if '"healthy"' in body:
                        print("[INFO] Graphiti server healthy.")
                        return True
            except Exception:
                pass
            time.sleep(2)
        print(f"[ERROR] Graphiti did not become healthy within {timeout_sec}s.")
        return False

    def info(self) -> EngineInfo:
        return EngineInfo(
            name="Graphiti",
            entries={
                "Container": self.container,
                "API": f"http://127.0.0.1:{self.port}",
                "API docs": f"http://127.0.0.1:{self.port}/docs",
                "Neo4j Browser": "http://127.0.0.1:7474",
                "Logs": os.path.join(self.rag_dir, "logs/"),
            },
        )
