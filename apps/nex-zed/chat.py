#!/usr/bin/env python3
"""
nex-zed: Helix headless Zed 대화형 채팅

Zed를 --headless 모드로 실행하고,
WebSocket으로 연결하여 AI 에이전트와 대화합니다.

사용법:
  ./chat.py                                    # 기본 실행
  ./chat.py --bin ../.bin/helix-zed-headless-arm64  # 바이너리 경로 지정
  ./chat.py --workdir /path/to/project              # 작업 디렉토리 지정

명령어:
  메시지를 입력하면 Zed 에이전트로 전송됩니다.
  /exit, /quit    - 종료
  /new            - 새 스레드 시작
  /thread         - 현재 스레드 ID 보기
  /raw            - 원시 JSON 메시지 표시 토글

예시:
  > 이 디렉토리에 뭐가 있어?
  > main.rs를 읽어줘
  > 여기에 TODO 주석이 있나 찾아봐
"""

import asyncio
import json
import os
import signal
import subprocess
import sys
import time
import uuid
from datetime import datetime, timezone

try:
    import websockets
except ImportError:
    print("Installing websockets...")
    subprocess.check_call([sys.executable, "-m", "pip", "install", "websockets"])
    import websockets


# ── 설정 ──────────────────────────────────────────────────────────────

HOST = "127.0.0.1"
WS_PORT = 8080
ZED_LOG = "/tmp/nex-zed-headless.log"
SESSION_ID = f"ses_nex-zed-{uuid.uuid4().hex[:8]}"

# 현재 스레드 ID (chat_message를 보내면 Zed가 응답으로 알려줌)
current_thread_id = None
current_request_id = None
# 원시 JSON 표시
show_raw = False
# 연결된 Zed 소켓
zed_ws = None


# ── ANSI 색상 ─────────────────────────────────────────────────────────

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


# ── WebSocket 서버: Zed의 연결을 받음 ────────────────────────────────

async def handle_zed(websocket):
    """Zed가 WebSocket으로 연결했을 때 호출됨."""
    global zed_ws, current_thread_id
    zed_ws = websocket
    peer = websocket.remote_address

    print(f"\n{C.GREEN}{C.BOLD}✓ Zed 연결됨 ({peer}){C.END}")
    print(f"{C.DIM}에이전트 준비 중... (최대 10초){C.END}")

    # /tmp/hl-stdin.sock 등 이전 소켓 파일 정리
    for sock in ["/tmp/hl-stdin.sock", "/tmp/hl-stdout.sock", "/tmp/hl-stderr.sock", "/tmp/hl.pid"]:
        try:
            os.remove(sock)
        except FileNotFoundError:
            pass

    # 잠시 기다리면 Zed가 agent_ready를 보냄
    try:
        async for raw in websocket:
            try:
                msg = json.loads(raw)
                await handle_message(msg)
            except json.JSONDecodeError:
                pass
    except websockets.exceptions.ConnectionClosed:
        print(f"\n{C.RED}Zed 연결 종료됨{C.END}")
    finally:
        zed_ws = None


async def handle_message(msg):
    """Zed로부터 받은 WebSocket 메시지 처리."""
    global current_thread_id, current_request_id
    event_type = msg.get("event_type", "unknown")
    data = msg.get("data", {})

    if show_raw:
        print(f"\n{C.DIM}[RAW] {json.dumps(msg, indent=2)}{C.END}")

    if event_type == "ping":
        # ping-pong
        await zed_ws.send(json.dumps({"type": "pong", "data": data}))

    elif event_type == "pong":
        pass  # 무시

    elif event_type == "agent_ready":
        agent = data.get("agent_name", "?")
        tid = data.get("thread_id")
        print(f"\n{C.GREEN}{C.BOLD}✓ 에이전트 준비 완료 ({agent}){C.END}")
        if tid:
            current_thread_id = tid
            print(f"  스레드: {C.CYAN}{tid}{C.END}")
        print(f"\n{C.BOLD}메시지를 입력하세요. /exit로 종료.{C.END}")
        print(f"{C.DIM}예: \"이 디렉토리에 뭐가 있어?\"{C.END}")

    elif event_type == "thread_created":
        tid = data.get("acp_thread_id", "?")
        rid = data.get("request_id", "")
        current_thread_id = tid
        print(f"\n{C.CYAN}📌 새 스레드 생성: {tid}{C.END}")
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
            # 스트리밍 출력 (줄바꿈 없이)
            print(content, end="", flush=True)

    elif event_type == "message_completed":
        mid = data.get("message_id", "?")
        print(f"\n{C.GREEN}✓ 완료 (message: {mid[:8]}){C.END}")
        print()

    elif event_type == "thread_load_error":
        error = data.get("error", "?")
        print(f"\n{C.RED}⚠ 스레드 로드 실패: {error}{C.END}")

    elif event_type == "turn_cancelled":
        status = data.get("status", "?")
        print(f"\n{C.YELLOW}⚠ 취소됨 ({status}){C.END}")

    elif event_type == "chat_response_error":
        error = data.get("error", "?")
        print(f"\n{C.RED}⚠ 응답 오류: {error}{C.END}")

    elif event_type == "user_created_thread":
        print(f"\n{C.CYAN}📌 사용자가 새 스레드 생성{C.END}")


# ── Zed로 명령 전송 ─────────────────────────────────────────────────

async def send_chat(message):
    """chat_message 명령을 Zed로 전송."""
    global current_request_id
    if not zed_ws:
        print(f"{C.RED}⚠ Zed가 아직 연결되지 않았습니다.{C.END}")
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
    """진행 중인 요청 취소."""
    if not zed_ws or not current_request_id:
        print(f"{C.YELLOW}⚠ 취소할 요청이 없습니다.{C.END}")
        return

    cmd = {
        "type": "cancel_current_turn",
        "data": {"request_id": current_request_id},
    }
    await zed_ws.send(json.dumps(cmd))
    print(f"{C.YELLOW}⚠ 취소 요청 전송{C.END}")


# ── stdin 입력 처리 ──────────────────────────────────────────────────

async def read_stdin():
    """사용자 입력을 읽어서 처리."""
    # stdin이 터미널이 아니면 조용히 리턴
    if not sys.stdin.isatty() or not sys.__stdin__ or not sys.__stdin__.isatty():
        return

    loop = asyncio.get_event_loop()
    reader = asyncio.StreamReader()
    protocol = asyncio.StreamReaderProtocol(reader)

    try:
        await loop.connect_read_pipe(lambda: protocol, sys.stdin)
    except (OSError, AttributeError):
        return  # 백그라운드 모드

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
            print("종료합니다.")
            os._exit(0)

        elif text == "/new":
            current_thread_id = None
            print(f"{C.CYAN}📌 새 스레드 모드 (기존 스레드에 이어서 않음){C.END}")

        elif text == "/thread":
            if current_thread_id:
                print(f"현재 스레드: {C.CYAN}{current_thread_id}{C.END}")
            else:
                print(f"{C.YELLOW}현재 스레드 없음 (새로 생성됩니다){C.END}")

        elif text == "/raw":
            show_raw = not show_raw
            print(f"원시 JSON 표시: {'ON' if show_raw else 'OFF'}")

        elif text == "/cancel":
            await send_cancel()

        elif text == "/help":
            print(f"{C.BOLD}명령어:{C.END}")
            print("  /exit, /quit   - 종료")
            print("  /new           - 새 스레드 시작")
            print("  /thread        - 현재 스레드 ID 보기")
            print("  /cancel        - 진행 중인 응답 취소")
            print("  /raw           - 원시 JSON 표시 토글")
            print("  /help          - 이 도움말")

        elif text.startswith("/"):
            print(f"{C.YELLOW}알 수 없는 명령어: {text}{C.END}")

        else:
            await send_chat(text)


# ── 메인 ──────────────────────────────────────────────────────────────

def main():
    import argparse

    parser = argparse.ArgumentParser(description="nex-zed: Helix headless Zed 채팅")
    parser.add_argument("--bin", default=None,
                        help="helix-zed-headless 바이너리 경로")
    parser.add_argument("--workdir", default=os.getcwd(),
                        help="작업 디렉토리 (기본: 현재 디렉토리)")
    parser.add_argument("--no-zed", action="store_true",
                        help="Zed를 실행하지 않고 WebSocket 서버만 시작")
    args = parser.parse_args()

    # 바이너리 경로 자동 탐색
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
            print(f"{C.RED}helix-zed-headless 바이너리를 찾을 수 없습니다.{C.END}")
            print(f"  --bin 옵션으로 경로를 지정하세요.")
            sys.exit(1)

    workdir = os.path.abspath(args.workdir)

    print(f"\n{C.BOLD}{C.HEADER}╔══════════════════════════════════════╗{C.END}")
    print(f"{C.BOLD}{C.HEADER}║        nex-zed: Helix 채팅          ║{C.END}")
    print(f"{C.BOLD}{C.HEADER}╚══════════════════════════════════════╝{C.END}")
    print(f"  {C.DIM}바이너리:{C.END} {bin_path or '(서버 전용)'}")
    print(f"  {C.DIM}작업 디렉토리:{C.END} {workdir}")
    print(f"  {C.DIM}세션 ID:{C.END} {SESSION_ID}")
    print(f"  {C.DIM}WebSocket:{C.END} ws://{HOST}:{WS_PORT}/api/v1/external-agents/sync")
    print()

    # 이전 프로세스 정리
    if not args.no_zed:
        subprocess.run(["pkill", "-f", "helix-zed-headless"], capture_output=True)
        time.sleep(1)

    # 로그 파일 정리
    if os.path.exists(ZED_LOG):
        os.remove(ZED_LOG)

    # ── async 메인 ──
    async def async_main():
        global zed_ws

        # WebSocket 서버 시작
        server = await websockets.serve(
            handle_zed,
            HOST,
            WS_PORT,
            process_request=lambda path, headers: None,
        )
        print(f"{C.GREEN}✓ WebSocket 서버 시작됨 (포트 {WS_PORT}){C.END}")
        print(f"{C.DIM}  Zed가 연결되길 기다리는 중...{C.END}")

        # Zed 프로세스 시작
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
                [bin_path, "--headless", "--allow-multiple-instances", workdir],
                env=env,
                stdout=subprocess.DEVNULL,
                stderr=open(ZED_LOG, "w"),
            )
            print(f"{C.GREEN}✓ Zed 시작됨 (PID: {zed_proc.pid}){C.END}")
            print(f"  {C.DIM}로그: {ZED_LOG}{C.END}")
            print()

        # stdin 입력 태스크 시작 (터미널이 아닌 경우 아무것도 안 함)
        asyncio.create_task(read_stdin())

        # Zed가 종료될 때까지 대기
        if zed_proc:
            loop = asyncio.get_event_loop()
            try:
                await loop.run_in_executor(None, zed_proc.wait)
            except asyncio.CancelledError:
                pass
            print(f"\n{C.YELLOW}Zed 프로세스 종료됨{C.END}")
        else:
            # 서버 전용 모드 or no-stdin: 계속 실행
            while True:
                await asyncio.sleep(5)
                # 주기적으로 상태 출력 (background mode)
                print(f".", end="", flush=True)

    try:
        asyncio.run(async_main())
    except KeyboardInterrupt:
        print(f"\n{C.YELLOW}종료 중...{C.END}")
    finally:
        # 정리
        subprocess.run(["pkill", "-f", "helix-zed-headless"], capture_output=True)
        print(f"{C.GREEN}✓ 정리 완료{C.END}")


if __name__ == "__main__":
    main()
