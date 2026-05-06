"""CLI entry point for run-rag."""

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


def _find_engine(name: str, rag_dir: str) -> AbstractEngine:
    cls = ENGINE_REGISTRY.get(name)
    if cls is None:
        print(
            f"[ERROR] Unknown engine: {name}.  Available: {', '.join(ENGINE_REGISTRY)}"
        )
        sys.exit(1)
    return cls(rag_dir)


def _build_parser() -> argparse.ArgumentParser:
    choices = list(ENGINE_REGISTRY)
    parser = argparse.ArgumentParser(
        prog="run-rag",
        description="RAG Launcher — unified entry point for all RAG engines",
    )
    parser.add_argument(
        "--engine",
        choices=choices,
        default=os.environ.get("ENGINE", "lightrag"),
        help="Select RAG engine (default: lightrag)",
    )
    parser.add_argument(
        "--refresh",
        action="store_true",
        help="Delete all data before starting",
    )
    return parser


def main() -> None:
    args = _build_parser().parse_args()

    script_dir = os.path.dirname(os.path.abspath(__file__))
    # runners/ lives inside rag/
    rag_dir = os.environ.get("RAG_DIR", os.path.dirname(script_dir))

    # ------------------------------------------------------------------
    # Header
    # ------------------------------------------------------------------
    print(f"\n{CYAN}============================================================{NC}")
    print(f"{CYAN}  RAG Launcher — Engine: {args.engine}{NC}")
    print(f"{CYAN}============================================================{NC}\n")

    # ------------------------------------------------------------------
    # Engine
    # ------------------------------------------------------------------
    engine = _find_engine(args.engine, rag_dir)

    # ------------------------------------------------------------------
    # Pre-flight
    # ------------------------------------------------------------------
    def cleanup() -> None:
        print("[INFO] Shutting down...")
        engine.stop()
        # tunnel manager is stopped via atexit / signal
        print("[INFO] All services stopped.")

    atexit.register(cleanup)
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda signum, frame: sys.exit(0))

    print("[INFO] Stopping any existing services...")
    engine.stop()
    # Skip cloudflared process kill — supervisor will just restart it

    TunnelManager.cleanup_stale_connections()

    # ------------------------------------------------------------------
    # Shared checks
    # ------------------------------------------------------------------
    lmstudio_url = os.environ.get("LMSTUDIO_URL", "http://localhost:1234")
    if not check_lm_studio(lmstudio_url):
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
    # Tunnel
    # ------------------------------------------------------------------
    os.makedirs(os.path.join(rag_dir, "logs"), exist_ok=True)
    tunnel = TunnelManager(engine.tunnel_config)
    tunnel.start()
    time.sleep(6)  # allow tunnel to register

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

    # Block until tunnel exits
    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        pass
    finally:
        tunnel.stop()
