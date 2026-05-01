FROM node:24-slim

ENV NODE_ENV=production \
    PIP_BREAK_SYSTEM_PACKAGES=1 \
    UV_SYSTEM_PYTHON=1

# Install system dependencies (Debian/Ubuntu compatible)
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 python3-pip python3-venv \
    git curl ca-certificates build-essential \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*

# Install uv (Python package installer)
COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /usr/local/bin/

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    --default-toolchain stable --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

# Install Python packages used by SwarmVault (embeddings, vector DB, crawling)
# RUN uv pip install --system --break-system-packages \
#     sentence-transformers \
#     chromadb \
#     numpy \
#     networkx \
#     requests \
#     markdownify \
#     beautifulsoup4

WORKDIR /work