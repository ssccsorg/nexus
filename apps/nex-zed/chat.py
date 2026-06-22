#!/usr/bin/env python3
"""
nex-zed: REST chat client for the Rust nex-zed server.

Connects to the Rust HTTP server at localhost:9090 which already manages
Zed headless.  No subprocess, no WebSocket, no API key needed here.

Usage:
  ./chat.py                              # default port 9090
  ./chat.py --port 9091                  # custom port
  ./chat.py --workdir /path/to/project   # no-zed fallback info
  ./chat.py --no-zed                     # fallback mode

Commands:
  /exit, /quit    - exit
  /new            - start a new thread
  /thread         - show current thread ID
  /raw            - toggle raw JSON message display

Examples:
  > What's in this directory?
  > Read main.rs
  > Find TODO comments here
"""

import argparse
import asyncio
import json
import os
import sys
from pathlib import Path

try:
    import httpx
except ImportError:
    import subprocess as _sp
    print("Installing httpx...")
    _sp.check_call([sys.executable, "-m", "pip", "install", "httpx"])
    import httpx


# ── Globals ──────────────────────────────────────────────────────────────

_shutdown = False
current_thread_id = None
show_raw = False


# ── ANSI colors ──────────────────────────────────────────────────────────

class C:
    HEADER = '\033[95m'
    BLUE = '\033[94m'
    CYAN = '\033[96m'
    GREEN = '\033[92m'
    YELLOW = '\033[93m'
    RED = '\033[91m'
    BOLD = '\033[1m'
    DIM = '\033[2m'
    END = '\033[0m'


# ── HTTP client ──────────────────────────────────────────────────────────

class NexClient:
    """Thin wrapper over the Rust server's REST API."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self.client = httpx.AsyncClient(base_url=self.base_url, timeout=30.0)

    async def close(self):
        await self.client.aclose()

    async def health(self) -> dict | None:
        try:
            r = await self.client.get("/health")
            r.raise_for_status()
            return r.json()
        except Exception:
            return None

    async def get_thread(self, thread_id: str) -> dict | None:
        try:
            r = await self.client.get(f"/v1/threads/{thread_id}")
            if r.status_code == 404:
                return None
            r.raise_for_status()
            return r.json()
        except httpx.HTTPStatusError:
            return None


# ── Message display ──────────────────────────────────────────────────────

def print_banner(h: dict):
    print(f"\n{C.BOLD}{C.HEADER}╔══════════════════════════════════════╗{C.END}")
    print(f"{C.BOLD}{C.HEADER}║       nex-zed: Server Chat           ║{C.END}")
    print(f"{C.BOLD}{C.HEADER}╚══════════════════════════════════════╝{C.END}")
    ok = h and h.get("status") == "ok"
    if ok:
        zed = h.get("zed_connected", False)
        agent = h.get("agent_ready", False)
        print(f"  {C.GREEN}✓{C.END} Server: {h.get('status', '?')}")
        print(f"  {C.GREEN}✓{C.END} Zed connected: {zed}")
        print(f"  {C.GREEN}✓{C.END} Agent ready: {agent}")
        print(f"  {C.DIM}Active threads: {h.get('active_threads', 0)}{C.END}")
        if not zed or not agent:
            print(f"\n{C.YELLOW}⚠ Waiting for Zed to connect...{C.END}")
    else:
        print(f"  {C.RED}✗{C.END} Server unreachable")
    print()


# ── Send message (SSE streaming) ─────────────────────────────────────────

async def send_chat(client: NexClient, message: str):
    """Send a message via SSE streaming endpoint /v1/chat."""
    global current_thread_id, show_raw

    if not message:
        return

    body = {"message": message}
    if current_thread_id:
        body["thread_id"] = current_thread_id

    if show_raw:
        print(f"\n{C.DIM}[RAW REQ] {json.dumps(body)}{C.END}")

    thread_announced = False

    url = f"{client.base_url}/v1/chat"
    body_str = json.dumps(body)
    proc = await asyncio.create_subprocess_exec(
        "curl", "-s", "-N", "-X", "POST", url,
        "-H", "Content-Type: application/json",
        "-d", body_str,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.DEVNULL
    )
    print(f"{C.DIM}[SSE] curl started PID {proc.pid}{C.END}", file=sys.stderr)

    event_type = ""
    raw_data = ""

    while not _shutdown:
        line_b = await proc.stdout.readline()
        if not line_b:
            break
        line = line_b.decode().strip()

        if line.startswith("event: "):
            event_type = line[7:].strip()
        elif line.startswith("data: "):
            raw_data = line[6:]
        elif not line:
            if not event_type or not raw_data:
                event_type = ""
                raw_data = ""
                continue

            try:
                event_data = json.loads(raw_data)
            except json.JSONDecodeError:
                event_type = ""
                raw_data = ""
                continue

            if show_raw:
                print(f"\n{C.DIM}[RAW SSE] event={event_type} data={json.dumps(event_data)}{C.END}")

            if event_type == "thread_created":
                new_tid = event_data.get("thread_id", "")
                if new_tid:
                    current_thread_id = new_tid
                if not thread_announced:
                    print(f"\n{C.CYAN}Thread: {current_thread_id}{C.END}\n")
                    thread_announced = True

            elif event_type == "message_added":
                delta = event_data.get("content", "")
                if delta:
                    print(delta, end="", flush=True)

            elif event_type == "message_completed":
                print(f"\n{C.GREEN}✓ Complete{C.END}")
                print()
                proc.kill()
                return

            event_type = ""
            raw_data = ""

    await proc.wait()

    if not thread_announced and current_thread_id:
        print(f"\n{C.CYAN}Thread: {current_thread_id}{C.END}\n")


# ── Stdin reader ─────────────────────────────────────────────────────────

async def read_stdin(client: NexClient):
    """Read user input from stdin and handle commands."""
    global _shutdown, current_thread_id, show_raw

    if not sys.stdin.isatty() or not sys.__stdin__ or not sys.__stdin__.isatty():
        return

    loop = asyncio.get_event_loop()
    reader = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(reader)

    try:
        await loop.connect_read_pipe(lambda: protocol, sys.stdin)
    except (OSError, AttributeError):
        return

    while not _shutdown:
        try:
            line = await reader.readline()
        except Exception:
            break
        if not line:
            await asyncio.sleep(0.1)
            continue

        text = line.decode().strip()
        if not text:
            continue

        if text in ("/exit", "/quit"):
            print("Exiting.")
            _shutdown = True
            return

        elif text == "/new":
            current_thread_id = None
            print(f"{C.CYAN}New thread mode (next message creates a fresh thread){C.END}")

        elif text == "/thread":
            if current_thread_id:
                print(f"Current thread: {C.CYAN}{current_thread_id}{C.END}")
            else:
                print(f"{C.YELLOW}No current thread (a new one will be created){C.END}")

        elif text == "/raw":
            show_raw = not show_raw
            print(f"Raw JSON display: {'ON' if show_raw else 'OFF'}")

        elif text == "/help":
            print(f"{C.BOLD}Commands:{C.END}")
            print("  /exit, /quit   - exit")
            print("  /new           - start a new thread")
            print("  /thread        - show current thread ID")
            print("  /raw           - toggle raw JSON display")
            print("  /help          - show this help")

        elif text.startswith("/"):
            print(f"{C.YELLOW}Unknown command: {text}{C.END}")

        else:
            await send_chat(client, text)


# ── Main ─────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="nex-zed: REST chat client for Rust nex-zed server")
    parser.add_argument("--port", type=int, default=9090,
                        help="Server port (default: 9090)")
    parser.add_argument("--workdir", default=os.getcwd(),
                        help="Working directory hint (for --no-zed fallback)")
    parser.add_argument("--no-zed", action="store_true",
                        help="Fallback mode: do not expect Zed to be managed")
    args = parser.parse_args()

    base_url = f"http://localhost:{args.port}"
    workdir = os.path.abspath(args.workdir)

    # Load .env file (informational only)
    env_file = Path(__file__).parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, _, val = line.partition("=")
                os.environ.setdefault(key.strip(), val.strip())

    async def async_main():
        client = NexClient(base_url)

        # Health check
        h = await client.health()
        print_banner(h)

        if h and h.get("status") == "ok":
            print(f"  {C.DIM}Server:{C.END} {base_url}")
            print(f"  {C.DIM}Workdir:{C.END} {workdir}")
            print()
            if h.get("zed_connected") and h.get("agent_ready"):
                print(f"{C.BOLD}Enter a message. /exit to quit.{C.END}")
                print(f"{C.DIM}Example: \"What's in this directory?\"{C.END}")

            asyncio.create_task(read_stdin(client))

            # Wait until shutdown
            while not _shutdown:
                await asyncio.sleep(1)

        await client.close()

    try:
        asyncio.run(async_main())
    except KeyboardInterrupt:
        print(f"\n{C.YELLOW}Shutdown{C.END}")
    finally:
        print(f"{C.GREEN}Done{C.END}")


if __name__ == "__main__":
    main()
