#!/usr/bin/env python3
"""
nex-zed: Helix headless Zed interactive chat

Runs Zed in --headless mode,
connects via WebSocket to converse with an AI agent.

Usage:
  ./chat.py                                    # default run
  ./chat.py --bin ../.bin/helix-zed-headless-arm64  # specify binary path
  ./chat.py --workdir /path/to/project              # specify working directory

Commands:
  Enter a message and it will be sent to the Zed agent.
  /exit, /quit    - exit
  /new            - start a new thread
  /thread         - show current thread ID
  /raw            - toggle raw JSON message display

Examples:
  > What's in this directory?
  > Read main.rs
  > Find TODO comments here
"""

import asyncio
import json
import os
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path

# Global shutdown flag — when set, main loop exits cleanly.
_shutdown = False

try:
    import websockets
except ImportError:
    print("Installing websockets...")
    subprocess.check_call([sys.executable, "-m", "pip", "install", "websockets"])
    import websockets


# ── Config ──────────────────────────────────────────────────────────────

HOST = "127.0.0.1"
WS_PORT = 8080
ZED_LOG = "/tmp/nex-zed-headless.log"
SESSION_ID = f"ses_nex-zed-{uuid.uuid4().hex[:8]}"

# Zed responds with thread_id when we send chat_message
current_thread_id = None
current_request_id = None
# Raw JSON display
show_raw = False
# Connected Zed socket
zed_ws = None


# ── ANSI colors ─────────────────────────────────────────────────────────

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


# ── WebSocket server: handles Zed connections ────────────────────────────────

async def handle_zed(websocket):
    """Called when Zed connects via WebSocket."""
    global zed_ws, current_thread_id
    zed_ws = websocket
    peer = websocket.remote_address

    print(f"\n{C.GREEN}{C.BOLD}✓ Zed connected ({peer}){C.END}")
    print(f"{C.DIM}Agent initializing... (up to 10s){C.END}")

    # Clean up old socket files like /tmp/hl-stdin.sock
    for sock in ["/tmp/hl-stdin.sock", "/tmp/hl-stdout.sock", "/tmp/hl-stderr.sock", "/tmp/hl.pid"]:
        try:
            os.remove(sock)
        except FileNotFoundError:
            pass

    # Wait briefly for Zed to send agent_ready
    try:
        async for raw in websocket:
            try:
                msg = json.loads(raw)
                await handle_message(msg)
            except json.JSONDecodeError:
                pass
    except websockets.exceptions.ConnectionClosed:
        print(f"\n{C.RED}Zed connection closed{C.END}")
    finally:
        zed_ws = None


async def handle_message(msg):
    """Handle WebSocket messages received from Zed."""
    global current_thread_id, current_request_id
    event_type = msg.get("event_type", "unknown")
    data = msg.get("data", {})

    if show_raw:
        print(f"\n{C.DIM}[RAW] {json.dumps(msg, indent=2)}{C.END}")

    if event_type == "ping":
        # ping-pong
        await zed_ws.send(json.dumps({"type": "pong", "data": data}))

    elif event_type == "pong":
        pass  # ignore

    elif event_type == "agent_ready":
        agent = data.get("agent_name", "?")
        tid = data.get("thread_id")
        print(f"\n{C.GREEN}{C.BOLD}✓ Agent ready ({agent}){C.END}")
        if tid:
            current_thread_id = tid
            print(f"  Thread: {C.CYAN}{tid}{C.END}")
        print(f"\n{C.BOLD}Enter a message. /exit to quit.{C.END}")
        print(f"{C.DIM}Example: \"What's in this directory?\"{C.END}")

    elif event_type == "thread_created":
        tid = data.get("acp_thread_id", "?")
        rid = data.get("request_id", "")
        current_thread_id = tid
        print(f"\n{C.CYAN}📌 New thread created: {tid}{C.END}")
        if show_raw:
            print(f"  request_id: {rid}")

    elif event_type == "message_added":
        content = data.get("content", "")
        role = data.get("role", "?")
        entry_type = data.get("entry_type", "text")
        tool_name = data.get("tool_name", "")
        tool_status = data.get("tool_status", "")

        if entry_type == "tool_call":
            if tool_status == "in_progress" or not tool_status:
                print(f"\n{C.YELLOW}🔧 {tool_name}{C.END}", end="", flush=True)
            elif tool_status == "completed":
                print(f" {C.GREEN}✓{C.END}", end="", flush=True)
            elif tool_status == "error":
                print(f" {C.RED}✗{C.END}", end="", flush=True)
        elif role == "assistant":
            # Streaming output (no newline)
            print(content, end="", flush=True)

    elif event_type == "message_completed":
        mid = data.get("message_id", "?")
        print(f"\n{C.GREEN}✓ Complete (message: {mid[:8]}){C.END}")
        print()

    elif event_type == "thread_load_error":
        error = data.get("error", "?")
        print(f"\n{C.RED}⚠ Thread load failed: {error}{C.END}")

    elif event_type == "turn_cancelled":
        status = data.get("status", "?")
        print(f"\n{C.YELLOW}⚠ Cancelled ({status}){C.END}")

    elif event_type == "chat_response_error":
        error = data.get("error", "?")
        print(f"\n{C.RED}⚠ Response error: {error}{C.END}")

    elif event_type == "user_created_thread":
        print(f"\n{C.CYAN}📌 User created a new thread{C.END}")


# ── Send commands to Zed ─────────────────────────────────────────────────

async def send_chat(message):
    """Send chat_message command to Zed."""
    global current_request_id
    if not zed_ws:
        print(f"{C.RED}⚠ Zed is not yet connected.{C.END}")
        return

    rid = uuid.uuid4().hex[:12]
    current_request_id = rid

    cmd = {
        "type": "chat_message",
        "data": {
            "message": message,
            "request_id": rid,
            "acp_thread_id": current_thread_id,
        },
    }

    if show_raw:
        print(f"\n{C.DIM}[SEND] {json.dumps(cmd, indent=2)}{C.END}")

    await zed_ws.send(json.dumps(cmd))


async def send_cancel():
    """Cancel ongoing request."""
    if not zed_ws or not current_request_id:
        print(f"{C.YELLOW}⚠ No request to cancel.{C.END}")
        return

    cmd = {
        "type": "cancel_current_turn",
        "data": {"request_id": current_request_id},
    }
    await zed_ws.send(json.dumps(cmd))
    print(f"{C.YELLOW}⚠ Cancel request sent.{C.END}")


# ── stdin input handling ──────────────────────────────────────────────────

async def read_stdin():
    """Read and process user input."""
    # Silently return if stdin is not a terminal
    if not sys.stdin.isatty() or not sys.__stdin__ or not sys.__stdin__.isatty():
        return

    loop = asyncio.get_event_loop()
    reader = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(reader)

    try:
        await loop.connect_read_pipe(lambda: protocol, sys.stdin)
    except (OSError, AttributeError):
        return  # background mode

    while True:
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
            global _shutdown
            _shutdown = True
            # Cancel the main waiter future if it exists
            for task in asyncio.all_tasks():
                if task.get_name() == "main-waiter":
                    task.cancel()
                    break
            return

        elif text == "/new":
            current_thread_id = None
            print(f"{C.CYAN}📌 New thread mode (will not continue existing thread){C.END}")

        elif text == "/thread":
            if current_thread_id:
                print(f"Current thread: {C.CYAN}{current_thread_id}{C.END}")
            else:
                print(f"{C.YELLOW}No current thread (a new one will be created){C.END}")

        elif text == "/raw":
            show_raw = not show_raw
            print(f"Raw JSON display: {'ON' if show_raw else 'OFF'}")

        elif text == "/cancel":
            await send_cancel()

        elif text == "/help":
            print(f"{C.BOLD}Commands:{C.END}")
            print("  /exit, /quit   - exit")
            print("  /new           - start a new thread")
            print("  /thread        - show current thread ID")
            print("  /cancel        - cancel ongoing response")
            print("  /raw           - toggle raw JSON display")
            print("  /help          - show this help")

        elif text.startswith("/"):
            print(f"{C.YELLOW}Unknown command: {text}{C.END}")

        else:
            await send_chat(text)


# ── Main ──────────────────────────────────────────────────────────────

# ── Zed settings file generation ─────────────────────────────────────────────

def ensure_settings(data_dir: str, api_key: str):
    """
    Create/verify Zed's settings.json to register the DeepSeek provider.
    """
    settings_dir = Path(data_dir) / "config"
    settings_dir.mkdir(parents=True, exist_ok=True)
    settings_file = settings_dir / "settings.json"

    settings = {}
    if settings_file.exists():
        try:
            settings = json.loads(settings_file.read_text())
        except (json.JSONDecodeError, OSError):
            pass

    # Register DeepSeek as an openai_compatible provider
    language_models = settings.setdefault("language_models", {})
    openai_compatible = language_models.setdefault("openai_compatible", {})

    if "deepseek" not in openai_compatible:
        openai_compatible["deepseek"] = {
            "api_url": "https://api.deepseek.com/v1",
            "available_models": [
                {
                    "name": "deepseek-chat",
                    "display_name": "DeepSeek V3",
                    "max_tokens": 65536,
                    "max_output_tokens": 8192,
                    "tool_use": True,
                }
            ],
        }

    # API key is set via Zed's keychain storage
    # Write directly to credentials.json under --user-data-dir
    creds_dir = Path(data_dir) / "credentials"
    creds_dir.mkdir(parents=True, exist_ok=True)
    creds_file = creds_dir / "credentials.json"

    creds = {}
    if creds_file.exists():
        try:
            creds = json.loads(creds_file.read_text())
        except (json.JSONDecodeError, OSError):
            pass

    # Save api_key at provider/deepseek path (Zed key format)
    creds["provider/deepseek"] = {
        "api_key": api_key
    }

    settings_file.write_text(json.dumps(settings, indent=2))
    creds_file.write_text(json.dumps(creds, indent=2))

    print(f"  {C.DIM}Settings:{C.END} {settings_file}")
    print(f"  {C.DIM}Credentials:{C.END} {creds_file}")
    return str(settings_dir.parent)  # return data_dir


def main():
    import argparse

    parser = argparse.ArgumentParser(description="nex-zed: Helix headless Zed chat")
    parser.add_argument("--bin", default=None,
                        help="helix-zed-headless binary path")
    parser.add_argument("--workdir", default=os.getcwd(),
                        help="Working directory (default: current directory)")
    parser.add_argument("--no-zed", action="store_true",
                        help="Start WebSocket server only, without launching Zed")
    parser.add_argument("--api-key", default=None,
                        help="DeepSeek API key (default: DEEPSEEK_API_KEY env var)")
    args = parser.parse_args()

    # Load .env file (if present)
    env_file = Path(__file__).parent / ".env"
    if env_file.exists():
        for line in env_file.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, _, val = line.partition("=")
                os.environ.setdefault(key.strip(), val.strip())

    # Check for DeepSeek API key (try multiple names)
    api_key = (args.api_key
               or os.environ.get("DEEPSEEK_API_KEY")
               or os.environ.get("LLM_API_KEY"))
    if not api_key:
        print(f"{C.RED}⚠ API key is required.{C.END}")
        print(f"  Use --api-key, DEEPSEEK_API_KEY, or LLM_API_KEY environment variable.")
        print(f"  You can also set LLM_API_KEY in apps/nex-zed/.env.")
        sys.exit(1)

    # Auto-detect binary path
    bin_path = args.bin
    if not bin_path and not args.no_zed:
        candidates = [
            os.path.expanduser("~/.bin/helix-zed-headless-arm64"),
            os.path.join(os.path.dirname(__file__), "..", ".bin", "helix-zed-headless-arm64"),
        ]
        for p in candidates:
            if os.path.exists(p):
                bin_path = os.path.abspath(p)
                break
        if not bin_path:
            print(f"{C.RED}Could not find helix-zed-headless binary.{C.END}")
            print(f"  Use --bin to specify the path.")
            sys.exit(1)

    workdir = os.path.abspath(args.workdir)

    # Create temporary user data directory + write settings
    user_data_dir = tempfile.mkdtemp(prefix="nex-zed-")
    ensure_settings(user_data_dir, api_key)

    print(f"\n{C.BOLD}{C.HEADER}╔══════════════════════════════════════╗{C.END}")
    print(f"{C.BOLD}{C.HEADER}║        nex-zed: Helix Chat          ║{C.END}")
    print(f"{C.BOLD}{C.HEADER}╚══════════════════════════════════════╝{C.END}")
    print(f"  {C.DIM}Binary:{C.END} {bin_path or '(server only)'}")
    print(f"  {C.DIM}Workdir:{C.END} {workdir}")
    print(f"  {C.DIM}Session ID:{C.END} {SESSION_ID}")
    print(f"  {C.DIM}User data:{C.END} {user_data_dir}")
    print(f"  {C.DIM}WebSocket:{C.END} ws://{HOST}:{WS_PORT}/api/v1/external-agents/sync")
    print()

    # Clean up previous processes
    if not args.no_zed:
        subprocess.run(["pkill", "-f", "helix-zed-headless"], capture_output=True)
        time.sleep(1)

    # Clean up log file
    if os.path.exists(ZED_LOG):
        os.remove(ZED_LOG)

    # ── async main ──
    async def async_main():
        global zed_ws

        async with websockets.serve(
            handle_zed,
            HOST,
            WS_PORT,
            process_request=lambda path, headers: None,
        ):
            print(f"{C.GREEN}✓ WebSocket server started (port {WS_PORT}){C.END}")
            print(f"{C.DIM}  Waiting for Zed to connect...{C.END}")

            zed_proc = None
            if not args.no_zed:
                env = os.environ.copy()
                env.update({
                    "ZED_EXTERNAL_SYNC_ENABLED": "true",
                    "ZED_WEBSOCKET_SYNC_ENABLED": "true",
                    "ZED_HELIX_URL": f"{HOST}:{WS_PORT}",
                    "ZED_HELIX_TOKEN": "test-token",
                    "HELIX_SESSION_ID": SESSION_ID,
                    "ZED_STATELESS": "1",
                    "RUST_LOG": "info",
                })

                zed_proc = subprocess.Popen(
                    [bin_path, "--headless", "--allow-multiple-instances",
                     "--user-data-dir", user_data_dir, workdir],
                    env=env,
                    stdout=subprocess.DEVNULL,
                    stderr=open(ZED_LOG, "w"),
                )
                print(f"{C.GREEN}✓ Zed started (PID: {zed_proc.pid}){C.END}")
                print(f"  {C.DIM}Log: {ZED_LOG}{C.END}")
                print()

            asyncio.create_task(read_stdin())

            async def wait_for_exit():
                nonlocal zed_proc
                if zed_proc:
                    loop = asyncio.get_event_loop()
                    await loop.run_in_executor(None, zed_proc.wait)
                else:
                    while not _shutdown:
                        await asyncio.sleep(1)

            waiter = asyncio.create_task(wait_for_exit(), name="main-waiter")
            try:
                await waiter
            except asyncio.CancelledError:
                pass

            if zed_proc and zed_proc.poll() is None:
                zed_proc.terminate()
                try:
                    await asyncio.wait_for(
                        asyncio.get_event_loop().run_in_executor(None, zed_proc.wait),
                        timeout=5.0
                    )
                except asyncio.TimeoutError:
                    zed_proc.kill()
                    await asyncio.get_event_loop().run_in_executor(None, zed_proc.wait)
                print(f"\n{C.YELLOW}Zed process terminated{C.END}")

        print(f"{C.GREEN}✓ Server shutdown complete{C.END}")

    try:
        asyncio.run(async_main())
    except KeyboardInterrupt:
        print(f"\n{C.YELLOW}Shutting down...{C.END}")
    finally:
        print(f"{C.GREEN}✓ Cleanup complete{C.END}")
        print(f"{C.DIM}  Settings file: {user_data_dir}/config/settings.json{C.END}")
        print(f"{C.DIM}  Can reuse with --user-data-dir {user_data_dir} on next run{C.END}")


if __name__ == "__main__":
    main()
