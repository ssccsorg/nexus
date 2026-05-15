# graphiti-server.py
# Thin HTTP wrapper exposing Graphiti's add_episode/retrieve_episodes/remove_episode
# as REST endpoints for the Cloudflare sync worker.
#
# Graphiti is a Python library (not a server), so we need this minimal wrapper
# to bridge the HTTP gap. The worker's GraphitiHandler calls these endpoints
# in the same way LightRagHandler calls LightRAG's built-in REST API.

import os
import sys
import traceback
import logging
from datetime import datetime, timezone
from contextlib import asynccontextmanager

import uvicorn
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

logging.basicConfig(level=logging.INFO, format="%(asctime)s [%(levelname)s] %(message)s")
logger = logging.getLogger("graphiti-server")

# --- Configuration ----------------------------------------------------------
NEO4J_URI = os.environ.get("NEO4J_URI", "bolt://neo4j:7687")
NEO4J_USER = os.environ.get("NEO4J_USER", "neo4j")
NEO4J_PASSWORD = os.environ.get("NEO4J_PASSWORD", "")

OPENAI_BASE_URL = os.environ.get("OPENAI_BASE_URL", "http://host.docker.internal:1234/v1")
OPENAI_API_KEY = os.environ.get("OPENAI_API_KEY", "lm-studio")
LLM_MODEL = os.environ["LLM_MODEL"]
EMBEDDING_MODEL = os.environ["EMBEDDING_MODEL"]
EMBEDDING_DIM = int(os.environ.get("EMBEDDING_DIM", "768"))

SERVER_HOST = os.environ.get("HOST", "0.0.0.0")
SERVER_PORT = int(os.environ.get("PORT", "8000"))
WORKSPACE = os.environ.get("WORKSPACE", "default")

# --- Graphiti instance (lazy init after DB is ready) -----------------------
graphiti_instance = None


async def init_graphiti():
    """Initialize Graphiti with OpenAI-compatible LLM and embedder."""
    global graphiti_instance
    from graphiti_core import Graphiti
    from graphiti_core.llm_client import OpenAIClient
    from graphiti_core.llm_client.config import LLMConfig
    from graphiti_core.embedder import OpenAIEmbedder
    from graphiti_core.embedder.openai import OpenAIEmbedderConfig

    logger.info(f"Connecting to Neo4j: {NEO4J_URI}")
    logger.info(f"LLM base URL: {OPENAI_BASE_URL}")
    logger.info(f"LLM model: {LLM_MODEL}")
    logger.info(f"Embedding model: {EMBEDDING_MODEL} ({EMBEDDING_DIM}d)")

    llm_client = OpenAIClient(
        config=LLMConfig(
            api_key=OPENAI_API_KEY,
            base_url=OPENAI_BASE_URL,
            model=LLM_MODEL,
        ),
    )
    embedder = OpenAIEmbedder(
        config=OpenAIEmbedderConfig(
            api_key=OPENAI_API_KEY,
            base_url=OPENAI_BASE_URL,
            embedding_model=EMBEDDING_MODEL,
            embedding_dim=EMBEDDING_DIM,
        ),
    )

    graphiti_instance = Graphiti(
        uri=NEO4J_URI,
        user=NEO4J_USER,
        password=NEO4J_PASSWORD,
        llm_client=llm_client,
        embedder=embedder,
    )

    await graphiti_instance.build_indices_and_constraints()
    logger.info("Graphiti initialized")


@asynccontextmanager
async def lifespan(app: FastAPI):
    logger.info("Starting Graphiti server...")
    try:
        await init_graphiti()
    except Exception as e:
        logger.error(f"Failed to initialize Graphiti: {e}")
        traceback.print_exc()
        sys.exit(1)
    yield
    if graphiti_instance:
        await graphiti_instance.close()
        logger.info("Graphiti connection closed")


app = FastAPI(title="Graphiti Server", lifespan=lifespan)


# --- Health Check -----------------------------------------------------------
@app.get("/health")
async def health():
    return {"status": "healthy"}


# --- Models -----------------------------------------------------------------
class EpisodeRequest(BaseModel):
    name: str
    episode_body: str
    source_description: str = "R2 sync"
    reference_time: str | None = None  # ISO 8601, defaults to now


@app.post("/episodes")
async def add_episode(req: EpisodeRequest):
    """Add a single episode to the temporal knowledge graph."""
    if graphiti_instance is None:
        raise HTTPException(status_code=503, detail="Graphiti not initialized")

    try:
        ref_time = (
            datetime.fromisoformat(req.reference_time)
            if req.reference_time
            else datetime.now(timezone.utc)
        )
        result = await graphiti_instance.add_episode(
            name=req.name,
            episode_body=req.episode_body,
            source_description=req.source_description,
            reference_time=ref_time,
            group_id=WORKSPACE,
        )
        return {"episode_uuid": result.episode.uuid}
    except Exception as e:
        logger.error(f"add_episode failed: {e}")
        traceback.print_exc()
        raise HTTPException(status_code=500, detail=str(e))


@app.get("/episodes")
async def list_episodes():
    """List recent episodes (last 100, for sync worker inventory check)."""
    if graphiti_instance is None:
        raise HTTPException(status_code=503, detail="Graphiti not initialized")

    try:
        episodes = await graphiti_instance.retrieve_episodes(
            reference_time=datetime.now(timezone.utc),
            last_n=100,
            group_ids=[WORKSPACE],
        )
        return {
            "episodes": [
                {
                    "uuid": ep.uuid,
                    "name": ep.name,
                    "content": ep.content[:200],
                    "created_at": ep.created_at.isoformat() if ep.created_at else None,
                }
                for ep in episodes
            ]
        }
    except Exception as e:
        logger.error(f"list_episodes failed: {e}")
        traceback.print_exc()
        raise HTTPException(status_code=500, detail=str(e))


@app.delete("/episodes/{episode_uuid}")
async def remove_episode(episode_uuid: str):
    """Remove an episode and its orphaned nodes/edges."""
    if graphiti_instance is None:
        raise HTTPException(status_code=503, detail="Graphiti not initialized")

    try:
        await graphiti_instance.remove_episode(episode_uuid)
        return {"status": "deleted", "episode_uuid": episode_uuid}
    except Exception as e:
        logger.error(f"remove_episode failed: {e}")
        traceback.print_exc()
        raise HTTPException(status_code=500, detail=str(e))


# --- Main ------------------------------------------------------------------
if __name__ == "__main__":
    uvicorn.run(app, host=SERVER_HOST, port=SERVER_PORT, log_level="info")
