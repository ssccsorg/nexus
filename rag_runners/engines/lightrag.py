"""LightRAG engine — runs as a single Docker container."""

import os
import shutil
import subprocess
import time
from urllib import request
from urllib.error import URLError

from ..base import AbstractEngine, EngineInfo
from ..checks import detect_embedding_dimension


class LightRAGEngine(AbstractEngine):
    name = "lightrag"

    def __init__(self, rag_dir: str) -> None:
        self.rag_dir = rag_dir
        self.image = os.environ.get("LIGHTRAG_IMAGE", "lightrag-nexus")
        self.container = os.environ.get("LIGHTRAG_CONTAINER", "lightrag-nexus")
        self.dockerfile = os.path.join(rag_dir, "Dockerfile.lightrag")
        self.port = int(os.environ.get("LIGHTRAG_PORT", "9621"))
        self.host = os.environ.get("LIGHTRAG_HOST", "0.0.0.0")
        self.data_dir = os.environ.get(
            "LIGHTRAG_DATA", os.path.join(rag_dir, "lightrag-data")
        )

    @property
    def tunnel_config(self) -> str:
        return os.path.join(self.rag_dir, "tunnel-config-lightrag.yml")

    def check(self) -> bool:
        if not shutil.which("docker"):
            print("[ERROR] Docker is required for LightRAG.")
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
            print(f"[INFO] Building LightRAG Docker image {self.image}...")
            subprocess.run(
                [
                    "docker",
                    "build",
                    "-t",
                    self.image,
                    "-f",
                    self.dockerfile,
                    self.rag_dir,
                ],
                check=True,
            )
        return True

    def start(self, refresh: bool = False) -> None:
        lmstudio_url = os.environ.get("LMSTUDIO_URL", "http://localhost:1234")
        llm_model = os.environ.get("LLM_MODEL", "liquid/lfm2.5-1.2b")
        embedding_model = os.environ.get(
            "EMBEDDING_MODEL", "text-embedding-nomic-embed-text-v1.5"
        )

        dim = detect_embedding_dimension(
            lmstudio_url,
            embedding_model,
            env_override=os.environ.get("EMBEDDING_DIMENSION"),
        )

        os.makedirs(self.data_dir, exist_ok=True)
        os.makedirs(os.path.join(self.rag_dir, "logs"), exist_ok=True)

        print(f"[INFO] LLM    : {llm_model}")
        print(f"[INFO] Embed  : {embedding_model} ({dim}d)")
        print(f"[INFO] Data   : {self.data_dir}")

        # Remove existing container
        subprocess.run(
            ["docker", "rm", "-f", self.container],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

        cmd = [
            "docker",
            "run",
            "-d",
            "--name",
            self.container,
            "--restart",
            "unless-stopped",
            "-p",
            f"{self.port}:9621",
            "-v",
            f"{self.data_dir}:/app/data",
            "-e",
            f"HOST={self.host}",
            "-e",
            "PORT=9621",
            "-e",
            f"LLM_BINDING_HOST={lmstudio_url}/v1",
            "-e",
            f"CHAT_MODEL={llm_model}",
            "-e",
            f"EMBEDDING_BINDING_HOST={lmstudio_url}/v1",
            "-e",
            f"EMBEDDING_MODEL={embedding_model}",
            "-e",
            f"EMBEDDING_DIM={dim}",
            "-e",
            "WORKING_DIR=/app/data",
            self.image,
            "--host",
            self.host,
            "--port",
            "9621",
            "--working-dir",
            "/app/data",
            "--workspace",
            "default",
            "--llm-binding",
            "openai",
            "--embedding-binding",
            "openai",
            "--log-level",
            "INFO",
        ]
        subprocess.run(cmd, check=True)

    def stop(self) -> None:
        subprocess.run(
            ["docker", "rm", "-f", self.container],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def health_check(self, timeout_sec: int) -> bool:
        print("[INFO] Waiting for LightRAG health check...")
        deadline = time.time() + timeout_sec
        while time.time() < deadline:
            try:
                req = request.Request(f"http://127.0.0.1:{self.port}/health")
                with request.urlopen(req, timeout=3) as resp:
                    body = resp.read().decode()
                    if '"healthy"' in body:
                        print("[INFO] LightRAG server healthy.")
                        return True
            except Exception:
                pass
            time.sleep(2)
        print(f"[ERROR] LightRAG did not become healthy within {timeout_sec}s.")
        return False

    def info(self) -> EngineInfo:
        return EngineInfo(
            name="LightRAG",
            entries={
                "Container": self.container,
                "API": f"http://127.0.0.1:{self.port}",
                "Web UI": f"http://127.0.0.1:{self.port}/webui",
                "API docs": f"http://127.0.0.1:{self.port}/docs",
                "Public": "https://rag-api.nexus.ssccs.org",
                "Logs": os.path.join(self.rag_dir, "logs/"),
            },
        )
