"""Shared dependency checks (LM Studio, cloudflared, embedding dimension)."""

import os
import platform
import shutil
import subprocess
import sys
from urllib import request
from urllib.error import URLError

import json

# ---------------------------------------------------------------------------
# LM Studio
# ---------------------------------------------------------------------------

def check_lm_studio(url: str = "http://localhost:1234") -> bool:
    print(f"  LM Studio ({url}) ... ", end="", flush=True)
    try:
        req = request.Request(f"{url}/v1/models")
        with request.urlopen(req, timeout=5) as resp:
            if resp.status == 200:
                print("OK")
                return True
    except Exception:
        pass
    print("\033[0;31mNOT REACHABLE\033[0m")
    print("    Start LM Studio and load a model first.")
    return False


# ---------------------------------------------------------------------------
# cloudflared
# ---------------------------------------------------------------------------

def check_cloudflared() -> bool:
    if shutil.which("cloudflared"):
        return True

    print("[WARN] cloudflared not found. Downloading...")

    system = platform.system()
    machine = platform.machine()

    os_map = {"Linux": "linux", "Darwin": "darwin"}
    arch_map = {"x86_64": "amd64", "arm64": "arm64", "aarch64": "arm64"}

    if system not in os_map:
        print(f"[ERROR] Unsupported OS: {system}")
        return False
    if machine not in arch_map:
        print(f"[ERROR] Unsupported arch: {machine}")
        return False

    dest = "/usr/local/bin/cloudflared"
    if os.path.isfile(dest):
        print(f"[INFO] {dest} already exists.")
        return True

    os_name = os_map[system]
    arch = arch_map[machine]
    url = (
        "https://github.com/cloudflare/cloudflared/releases/latest/"
        f"download/cloudflared-{os_name}-{arch}"
    )

    try:
        subprocess.run(["curl", "-fsSL", url, "-o", "cloudflared"], check=True)
        os.chmod("cloudflared", 0o755)
        try:
            subprocess.run(["sudo", "mv", "cloudflared", dest], check=True)
        except subprocess.CalledProcessError:
            shutil.move("cloudflared", "./cloudflared")
            os.environ["PATH"] = f"{os.getcwd()}:{os.environ['PATH']}"
        print("[INFO] cloudflared installed.")
        return True
    except subprocess.CalledProcessError as exc:
        print(f"[ERROR] Failed to download cloudflared: {exc}")
        return False


# ---------------------------------------------------------------------------
# Embedding dimension detection
# ---------------------------------------------------------------------------

def detect_embedding_dimension(
    base_url: str,
    model: str,
    env_override: str | None = None,
) -> int:
    """Probe the embedding API to determine vector dimension."""
    if env_override and env_override.strip():
        try:
            return int(env_override)
        except ValueError:
            pass

    print(f"[INFO] Probing embedding dimension for {model}...")
    payload = json.dumps({"model": model, "input": "test"}).encode()
    req = request.Request(
        f"{base_url}/v1/embeddings",
        data=payload,
        headers={"Content-Type": "application/json"},
    )
    try:
        with request.urlopen(req, timeout=15) as resp:
            data = json.loads(resp.read())
            dim = len(data["data"][0]["embedding"])
            print(f"[INFO] Detected embedding dimension: {dim}")
            return dim
    except Exception:
        print("[WARN] Could not detect dimension, falling back to 768")
        return 768
