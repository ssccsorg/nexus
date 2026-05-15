"""Unified CLI for all external Blackboard engines.

Usage:
  python -m _runner --engine lightrag
  python -m _runner --engine memgraph
"""

import argparse
import atexit
import os
import signal
import sys
import time

from .base import AbstractEngine
from .checks import check_cloudflared, check_lm_studio
from .engines import ALL as ENGINE_REGISTRY
from .tunnel import TunnelManager

CYAN = "\033[0;36m"
NC = "\033[0m"

# Engines that run via Docker and need LM Studio + tunnel
RAG_ENGINES = {"lightrag", "edgequake", "graphiti"}


def _find_engine(name: str) -> AbstractEngine:
    cls = ENGINE_REGISTRY.get(name)
    if cls is None:
        print(f"[ERROR] Unknown engine: {name}. Available: {', '.join(ENGINE_REGISTRY)}")
        sys.exit(1)
    return cls(os.environ.get("EXT_DIR", os.getcwd()))


def _build_parser() -> argparse.ArgumentParser:
    choices = list(ENGINE_REGISTRY)
    parser = argparse.ArgumentParser(
        prog="ext-run",
        description="External Blackboard engine launcher",
    )
    parser.add_argument(
        "--engine",
        choices=choices,
        default=os.environ.get("ENGINE", "lightrag"),
        help="Select engine (default: lightrag from $ENGINE)",
    )
    parser.add_argument(
        "--refresh",
        action="store_true",
        help="Delete all data before starting",
    )
    return parser


def main() -> None:
    args = _build_parser().parse_args()
    engine_name = args.engine
    engine = _find_engine(engine_name)

    # ------------------------------------------------------------------
    # Header
    # ------------------------------------------------------------------
    print(f"\n{CYAN}============================================================{NC}")
    print(f"{CYAN}  ext — Engine: {engine_name}{NC}")
    print(f"{CYAN}============================================================{NC}\n")

    # ------------------------------------------------------------------
    # Pre-flight
    # ------------------------------------------------------------------
    def cleanup() -> None:
        print("[INFO] Shutting down...")
        engine.stop()
        print("[INFO] All services stopped.")

    atexit.register(cleanup)
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda signum, frame: sys.exit(0))

    print("[INFO] Stopping any existing services...")
    engine.stop()

    # ------------------------------------------------------------------
    # Shared checks (RAG engines only)
    # ------------------------------------------------------------------
    if engine_name in RAG_ENGINES:
        TunnelManager.cleanup_stale_connections()
        api_base_url = os.environ.get(
            "LMSTUDIO_URL",
            os.environ.get("API_BASE_URL", "http://localhost:1234"),
        )
        if not check_lm_studio(api_base_url):
            sys.exit(1)
        if not check_cloudflared():
            sys.exit(1)

    # ------------------------------------------------------------------
    # Engine-specific check + start
    # ------------------------------------------------------------------
    if not engine.check():
        sys.exit(1)

    if args.refresh:
        print("[WARN] Refresh: clearing data.")
    engine.start(refresh=args.refresh)

    # ------------------------------------------------------------------
    # Health check
    # ------------------------------------------------------------------
    timeout_sec = int(os.environ.get("TIMEOUT_SEC", "120"))
    if not engine.health_check(timeout_sec):
        sys.exit(1)

    # ------------------------------------------------------------------
    # Tunnel (RAG engines only)
    # ------------------------------------------------------------------
    tunnel = None
    if engine_name in RAG_ENGINES:
        os.makedirs(os.path.join(os.environ.get("EXT_DIR", os.getcwd()), "logs"), exist_ok=True)
        tunnel = TunnelManager(engine.tunnel_config)
        tunnel.start()
        time.sleep(6)

    # ------------------------------------------------------------------
    # Ready
    # ------------------------------------------------------------------
    info = engine.info()
    print()
    print("============================================================")
    print("  Server is ready to accept connections")
    print("============================================================")
    print()
    for label, value in info.entries.items():
        print(f"  {label + ':':<11} {value}")
    print()

    # Block
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        if tunnel:
            tunnel.stop()
