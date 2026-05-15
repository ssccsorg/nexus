FROM python:3.11-slim

WORKDIR /work

COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/
RUN uv pip install --system --no-cache-dir \
    "graphiti-core>=0.6" \
    "fastapi>=0.115" \
    "uvicorn[standard]>=0.34"

COPY graphiti_server.py /work/

EXPOSE 8000

# All configuration supplied via environment at runtime (see runners/defaults.py)
ENV OPENAI_BASE_URL="" \
    OPENAI_API_KEY="" \
    LLM_MODEL="" \
    EMBEDDING_MODEL="" \
    EMBEDDING_DIM="768" \
    NEO4J_URI="bolt://neo4j:7687" \
    NEO4J_USER="neo4j" \
    NEO4J_PASSWORD="graphiti" \
    WORKSPACE="default"

CMD ["python", "/work/graphiti_server.py"]
