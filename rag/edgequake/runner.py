"""EdgeQuake engine — prebuilt GHCR images via docker compose profile."""

import os
import shutil
import subprocess
import time
from urllib import request

from runners.base import AbstractEngine, EngineInfo
from runners.checks import detect_embedding_dimension


class EdgeQuakeEngine(AbstractEngine):
    @property
    def name(self) -> str:
        return "edgequake"

    profile = "edgequake"

    def __init__(self, rag_dir: str) -> None:
        self.rag_dir = rag_dir
        self.engine_dir = os.path.join(rag_dir, "edgequake")
        self.compose_file = os.path.join(rag_dir, "docker-compose.yml")
        self.api_port = int(os.environ.get("EDGEQUAKE_PORT", "8080"))

    @property
    def tunnel_config(self) -> str:
        return os.path.join(self.engine_dir, "tunnel-config.yml")

    def _compose(self, *args: str, **kwargs) -> subprocess.CompletedProcess:
        cmd = ["docker", "compose", "--profile", self.profile, "-f", self.compose_file]
        cmd.extend(args)
        return subprocess.run(cmd, cwd=self.rag_dir, **kwargs)

    def check(self) -> bool:
        if not shutil.which("docker"):
            print("[ERROR] Docker is required for EdgeQuake.")
            return False
        result = subprocess.run(
            ["docker", "compose", "version"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        if result.returncode != 0:
            print("[ERROR] Docker Compose v2 is required.")
            return False
        if not os.path.isfile(self.compose_file):
            print(f"[ERROR] Compose file not found: {self.compose_file}")
            return False
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

        # extract port for Docker host.docker.internal mapping
        lmstudio_port = lmstudio_url.split(':')[-1] if ':' in lmstudio_url else '1234'

        env = os.environ.copy()
        env.update({
            "LMSTUDIO_URL": lmstudio_url,
            "LMSTUDIO_PORT": lmstudio_port,
            "LLM_MODEL": llm_model,
            "EMBEDDING_MODEL": embedding_model,
            "EMBEDDING_DIM": str(dim),
        })

        action = "down -v" if refresh else "down"
        self._compose(action, env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

        print("[INFO] Starting EdgeQuake (prebuilt)...")
        self._compose("up", "-d", "--wait", env=env, check=True)

        print(f"[INFO] LLM    : {llm_model}")
        print(f"[INFO] Embed  : {embedding_model} ({dim}d)")

    def stop(self) -> None:
        self._compose("down", stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

    def health_check(self, timeout_sec: int) -> bool:
        print("[INFO] Waiting for EdgeQuake health check...")
        deadline = time.time() + timeout_sec
        while time.time() < deadline:
            try:
                req = request.Request(f"http://127.0.0.1:{self.api_port}/health")
                with request.urlopen(req, timeout=3) as resp:
                    if 200 <= resp.status < 300:
                        print("[INFO] EdgeQuake API is healthy.")
                        return True
            except Exception:
                pass
            time.sleep(2)
        print(f"[ERROR] EdgeQuake did not become healthy within {timeout_sec}s.")
        return False

    def info(self) -> EngineInfo:
        return EngineInfo(
            name="EdgeQuake",
            entries={
                "API": f"http://127.0.0.1:{self.api_port}",
                "Web UI": f"http://127.0.0.1:{os.environ.get('FRONTEND_PORT', '3000')}",
                "Public": "https://rag-api.nexus.ssccs.org",
                "Logs": os.path.join(self.rag_dir, "logs/"),
            },
        )
