FROM python:3.11-slim

WORKDIR /app

COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/
RUN uv pip install --system --no-cache-dir "lightrag-hku[api]"

EXPOSE 9621

ENV LLM_BINDING=openai \
    LLM_BINDING_API_KEY=lm-studio \
    EMBEDDING_BINDING=openai \
    EMBEDDING_BINDING_API_KEY=lm-studio

ENTRYPOINT ["python", "-m", "lightrag.api.lightrag_server"]
