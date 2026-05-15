"""Graph Launcher CLI."""

import argparse
import atexit
import os
import signal
import sys
import time

from .base import AbstractEngine
from .engines import ALL as ENGINE_REGISTRY

CYAN = "\033[0;36m"
NC = "\033[0m"


def _find_engine(name: str, graph_dir: str) -> AbstractEngine:
    cls = ENGINE_REGISTRY.get(name)
    if cls is None:
        print(f"[ERROR] Unknown engine: {name}. Available: {', '.join(ENGINE_REGISTRY)}")
        sys.exit(1)
    return cls(graph_dir)


def main() -> None:
    choices = list(ENGINE_REGISTRY)
    parser = argparse.ArgumentParser(prog="graph-run", description="Graph Engine Launcher")
    parser.add_argument("--engine", choices=choices, default="memgraph", help="Graph engine")
    parser.add_argument("--refresh", action="store_true", help="Reset all data")
    args = parser.parse_args()

    script_dir = os.path.dirname(os.path.abspath(__file__))
    graph_dir = os.environ.get("GRAPH_DIR", os.path.dirname(script_dir))

    print(f"\n{CYAN}============================================================{NC}")
    print(f"{CYAN}  Graph Launcher — Engine: {args.engine}{NC}")
    print(f"{CYAN}============================================================{NC}\n")

    engine = _find_engine(args.engine, graph_dir)

    def cleanup() -> None:
        engine.stop()

    atexit.register(cleanup)
    for sig in (signal.SIGINT, signal.SIGTERM):
        signal.signal(sig, lambda signum, frame: sys.exit(0))

    engine.stop()

    if not engine.check():
        sys.exit(1)

    if args.refresh:
        print("[WARN] Refresh: clearing data.")
    engine.start(refresh=args.refresh)

    timeout_sec = int(os.environ.get("TIMEOUT_SEC", "30"))
    if not engine.health_check(timeout_sec):
        sys.exit(1)

    info = engine.info()
    print()
    print("============================================================")
    print("  Server is ready")
    print("============================================================")
    print()
    for label, value in info.entries.items():
        print(f"  {label + ':':<11} {value}")
    print()

    try:
        while True:
            time.sleep(1)
    except KeyboardInterrupt:
        pass
