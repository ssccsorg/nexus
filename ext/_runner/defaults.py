"""
Shared defaults for all RAG engines.

Import this module to get consistent defaults across lightrag, edgequake, and graphiti.
Override any value by setting the corresponding environment variable before starting.

Environment selection:
  Set DEPLOYMENT_ENV to "dev" or "prod" to switch default targets.
  When unset, "dev" is assumed.
"""

import os

# ---- Deployment environment ----
DEPLOYMENT_ENV = os.environ.get("DEPLOYMENT_ENV", "dev")

# ---- LLM ----
LLM_MODEL = os.environ.get("LLM_MODEL")

# ---- Embedding ----
EMBEDDING_MODEL = os.environ.get("EMBEDDING_MODEL")
EMBEDDING_DIM = (
    int(os.environ["EMBEDDING_DIM"])
    if "EMBEDDING_DIM" in os.environ
    else None
)

# ---- API Base URL (OpenAI-compatible backend) ----
API_BASE_URL = os.environ.get("API_BASE_URL")

# ---- Optional: separate chat model for engines that distinguish chat vs. extraction ----
CHAT_MODEL = os.environ.get("CHAT_MODEL")

# ---------------------------------------------------------------------------
# Per-environment defaults (only applied when env var is NOT set)
# ---------------------------------------------------------------------------
if DEPLOYMENT_ENV == "prod":
    LLM_MODEL = LLM_MODEL or "deepseek-chat"
    EMBEDDING_MODEL = EMBEDDING_MODEL or "text-embedding-3-small"
    EMBEDDING_DIM = EMBEDDING_DIM if EMBEDDING_DIM is not None else 1536
    API_BASE_URL = API_BASE_URL or os.environ.get(
        "OPENAI_BASE_URL", "https://api.deepseek.com"
    )
else:
    # dev / local defaults
    LLM_MODEL = LLM_MODEL or "qwen2.5-coder-7b-instruct-mlx"
    EMBEDDING_MODEL = EMBEDDING_MODEL or "text-embedding-nomic-embed-text-v1.5"
    EMBEDDING_DIM = EMBEDDING_DIM if EMBEDDING_DIM is not None else 768
    API_BASE_URL = API_BASE_URL or os.environ.get(
        "LMSTUDIO_URL", "http://host.docker.internal:1234"
    )

CHAT_MODEL = CHAT_MODEL or LLM_MODEL

# Backward-compat alias
LMSTUDIO_URL = API_BASE_URL

# ---- Deployment info for logging ----
print(f"[defaults] DEPLOYMENT_ENV={DEPLOYMENT_ENV}")
print(f"[defaults] LLM_MODEL={LLM_MODEL}")
print(f"[defaults] EMBEDDING_MODEL={EMBEDDING_MODEL} ({EMBEDDING_DIM}d)")
print(f"[defaults] API_BASE_URL={API_BASE_URL}")
