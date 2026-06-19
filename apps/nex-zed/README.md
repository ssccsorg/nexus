# nex-zed

neXus instance embedding ACP (Agent Client Protocol) as one of its communication surfaces.

## Architecture

```
Zed (네이티브 GUI)
  └── ACP (stdin/stdout) ──→ nex-zed (네이티브 또는 WASM)
                                └── neXus FIH ──→ Blackboard ←→ nex-cf (KG)

배포 모드:
  - 네이티브: 로컬 개발/테스트용
  - WASM: Cloudflare Workers 배포용 (nex-cf와 동일 인프라)
```

## Quick Start

```bash
# Build native binary
./scripts/deploy.sh --native

# Register in Zed settings.json:
# {
#   "agent_servers": {
#     "nex-zed": {
#       "type": "custom",
#       "command": "/path/to/target/release/nex-zed"
#     }
#   }
# }
```

## Scripts

```bash
./scripts/deploy.sh              # WASM build (default)
./scripts/deploy.sh --native     # Native binary build
./scripts/deploy.sh --deploy     # WASM + Cloudflare deploy
./scripts/deploy.sh --setup      # Initial setup
./scripts/deploy.sh --status     # Status check
```

## Project Structure

```
apps/nex-zed/
├── src/main.rs          # ACP 런처 (네이티브)
├── acp-bridge/          # Subtree: 독립 ACP 에이전트 서버
├── scripts/deploy.sh    # 배포/통합 스크립트
└── Cargo.toml

gateway/nex-zed-cf/      # WASM 배포용 (Cloudflare Workers)
└── build/nex-zed.wasm   # 빌드된 WASM 바이너리
```

## References

- Design: https://docs.ssccs.org/projects/nexus/apps/zed.llms.md
- Issue: https://github.com/ssccsorg/nexus/issues/72
