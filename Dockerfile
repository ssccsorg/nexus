FROM node:24-slim

ENV NODE_ENV=production \
    UV_SYSTEM_PYTHON=1 \
    UV_TOOL_BIN_DIR=/usr/local/bin \
    UV_TOOL_DIR=/opt/uv-tools

# Copy uv binary (replaces pip entirely)
COPY --from=ghcr.io/astral-sh/uv:latest /uv /uvx /bin/

# Install system dependencies (Debian)
RUN apt-get update && apt-get install -y --no-install-recommends \
    python3 \
    git curl ca-certificates build-essential \
    && apt-get clean -y && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    --default-toolchain stable --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /work
