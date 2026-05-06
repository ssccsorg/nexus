"""EdgeQuake engine — multi-container via docker compose."""

import os
import shutil
import subprocess
import time
from urllib import request
from urllib.error import URLError

from ..base import AbstractEngine, EngineInfo
from ..checks import detect_embedding_dimension


class EdgeQuakeEngine(AbstractEngine):
    name = "edgequake"

    def __init__(self, rag_dir: str) -> None:
        self.rag_dir = rag_dir
        repo_dir = os.environ.get(
            "EDGEQUAKE_REPO_DIR", os.path.join(rag_dir, "edgequake")
        )
        self.compose_file = os.path.join(
            repo_dir, "edgequake", "docker", "docker-compose.yml"
        )
        self.api_port = int(os.environ.get("EDGEQUAKE_PORT", "8080"))
        self.frontend_port = int(os.environ.get("FRONTEND_PORT", "3000"))

    @property
    def tunnel_config(self) -> str:
        return os.path.join(self.rag_dir, "tunnel-config-edgequake.yml")

    @property
    def _compose_dir(self) -> str:
        return os.path.dirname(self.compose_file)

    @property
    def _compose_basename(self) -> str:
        return os.path.basename(self.compose_file)

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

        env = os.environ.copy()
        env.update(
            {
                "EDGEQUAKE_LLM_PROVIDER": "openai",
                "EDGEQUAKE_LLM_MODEL": llm_model,
                "OPENAI_BASE_URL": f"{lmstudio_url}/v1",
                "OPENAI_API_KEY": "not-needed",
                "EDGEQUAKE_EMBEDDING_PROVIDER": "openai",
                "EDGEQUAKE_EMBEDDING_MODEL": embedding_model,
                "EDGEQUAKE_EMBEDDING_BASE_URL": f"{lmstudio_url}/v1",
                "EDGEQUAKE_EMBEDDING_DIMENSION": str(dim),
            }
        )

        compose_cmd = ["docker", "compose", "-f", self._compose_basename]

        if refresh:
            print("[WARN] Refresh mode: removing containers and volumes.")
            subprocess.run(
                compose_cmd + ["down", "-v"],
                cwd=self._compose_dir,
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
        else:
            subprocess.run(
                compose_cmd + ["down"],
                cwd=self._compose_dir,
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )

        print("[INFO] Starting EdgeQuake (Docker)...")
        subprocess.run(
            compose_cmd + ["up", "-d", "--build"],
            cwd=self._compose_dir,
            env=env,
            check=True,
        )

        print(f"[INFO] LLM    : {llm_model}")
        print(f"[INFO] Embed  : {embedding_model} ({dim}d)")

    def stop(self) -> None:
        subprocess.run(
            ["docker", "compose", "-f", self._compose_basename, "down"],
            cwd=self._compose_dir,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

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
                "Web UI": f"http://127.0.0.1:{self.frontend_port}",
                "Public": "https://rag-api.nexus.ssccs.org",
                "Logs": os.path.join(self.rag_dir, "logs/"),
            },
        )
