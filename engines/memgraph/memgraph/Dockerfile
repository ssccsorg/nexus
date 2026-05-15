FROM python:3.11-slim

WORKDIR /app

COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/
RUN uv pip install --system --no-cache-dir neo4j

COPY server.py .

EXPOSE 7689

ENV MEMGRAPH_HOST=memgraph \
    MEMGRAPH_PORT=7687 \
    PROXY_PORT=7689

CMD ["python", "server.py"]
