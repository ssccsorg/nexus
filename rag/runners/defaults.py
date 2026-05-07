"""
Shared defaults for all RAG engines.

Import this module to get consistent defaults across lightrag, edgequake, and graphiti.
Override any value by setting the corresponding environment variable before starting.
"""

import os

# ---- LLM ----
LLM_MODEL = os.environ.get("LLM_MODEL", "liquid/lfm2.5-1.2b")

# ---- Embedding ----
EMBEDDING_MODEL = os.environ.get(
    "EMBEDDING_MODEL", "text-embedding-nomic-embed-text-v1.5"
)
EMBEDDING_DIM = int(os.environ.get("EMBEDDING_DIM", "768"))

# ---- LM Studio (shared OpenAI-compatible backend) ----
LMSTUDIO_URL = os.environ.get(
    "LMSTUDIO_URL", "http://host.docker.internal:1234"
)

# ---- Optional: separate chat model for engines that distinguish chat vs. extraction ----
CHAT_MODEL = os.environ.get("CHAT_MODEL", LLM_MODEL)
