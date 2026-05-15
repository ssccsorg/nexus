#!/usr/bin/env python3
"""
fetch_ssccs.py — Universal sync router between SSCCS repositories.

Designed as an extensible transport+transform pipeline.
Each sync operation is a route: source → transform(s) → sink.

Current routes:
  nexus.llms → README.md  (docs.ssccs.org → local file, image path rewrite)

Future routes:
  R2 artifacts → gateway deployment
  IPFS CIDs → local cache
  GitHub issues → nexus Facts
  CI results → ssccs docs

Usage:
  ./fetch_ssccs.py                          # run default route
  ./fetch_ssccs.py --route nexus-readme     # explicit route
  ./fetch_ssccs.py --list-routes            # show available routes
"""

import argparse
import json
import os
import re
import sys
from abc import ABC, abstractmethod
from dataclasses import dataclass, field
from typing import Optional
from urllib.request import Request, urlopen

# ── Transport layer ──────────────────────────────────────────────────────

class Transport(ABC):
    """Fetch raw bytes from a source."""
    @abstractmethod
    def fetch(self, uri: str) -> bytes: ...

class HttpTransport(Transport):
    def fetch(self, uri: str) -> bytes:
        req = Request(uri, headers={"User-Agent": "ssccs-sync/0.1"})
        with urlopen(req, timeout=30) as resp:
            return resp.read()

class FileTransport(Transport):
    def fetch(self, uri: str) -> bytes:
        with open(uri, "rb") as f:
            return f.read()

# Future: R2Transport, IpfsTransport, GhApiTransport...

TRANSPORTS: dict[str, Transport] = {
    "http": HttpTransport(),
    "https": HttpTransport(),
    "file": FileTransport(),
}

def resolve_transport(uri: str) -> tuple[Transport, str]:
    """Pick transport by URI scheme."""
    scheme = uri.split("://", 1)[0] if "://" in uri else "file"
    transport = TRANSPORTS.get(scheme)
    if not transport:
        raise ValueError(f"No transport for scheme: {scheme}")
    return transport, uri

# ── Transform layer ──────────────────────────────────────────────────────

class Transform(ABC):
    """Transform raw bytes into output bytes."""
    name: str = "unnamed"

    @abstractmethod
    def apply(self, data: bytes, ctx: dict) -> bytes: ...

class ImagePathRewrite(Transform):
    """Rewrite relative image paths to absolute URLs."""

    name = "image-path-rewrite"
    IMAGE_EXTS = (".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp")

    def __init__(self, base_url: str = "https://docs.ssccs.org/projects/nexus/"):
        self.base_url = base_url

    def apply(self, data: bytes, ctx: dict) -> bytes:
        base = ctx.get("base_url", self.base_url)
        text = data.decode("utf-8")

        def replace(m):
            alt, path = m.group(1), m.group(2)
            if path.startswith("http") or path.startswith("//"):
                return m.group(0)
            if not any(path.lower().endswith(e) for e in self.IMAGE_EXTS):
                return m.group(0)
            absolute = base.rstrip("/") + "/" + path.lstrip("/")
            return f"![{alt}]({absolute})"

        return re.sub(r'!\[([^\]]*)\]\(([^)]+)\)', replace, text).encode("utf-8")

class PrependHeader(Transform):
    """Prepend a YAML-style header comment."""
    name = "prepend-header"

    def __init__(self, header: str = ""):
        self.header = header

    def apply(self, data: bytes, ctx: dict) -> bytes:
        hdr = ctx.get("header", self.header)
        return (hdr + "\n\n").encode("utf-8") + data if hdr else data

class StripFrontmatter(Transform):
    """Remove YAML frontmatter (--- ... ---)."""
    name = "strip-frontmatter"

    def apply(self, data: bytes, ctx: dict) -> bytes:
        text = data.decode("utf-8")
        stripped = re.sub(r'^---\n.*?\n---\n', '', text, count=1, flags=re.DOTALL)
        return stripped.encode("utf-8")

# ── Sink layer ───────────────────────────────────────────────────────────

class Sink(ABC):
    """Write (or send) the final bytes somewhere."""
    @abstractmethod
    def write(self, data: bytes, ctx: dict) -> None: ...

class FileSink(Sink):
    def __init__(self, path: str = "README.md"):
        self.path = path

    def write(self, data: bytes, ctx: dict) -> None:
        path = ctx.get("output_path", self.path)
        with open(path, "wb") as f:
            f.write(data)
        print(f"[sink] Written {path} ({len(data)} bytes)")

class StdoutSink(Sink):
    def write(self, data: bytes, ctx: dict) -> None:
        sys.stdout.buffer.write(data)

# Future: PrSink, IssueSink, R2Sink...

# ── Route definitions ────────────────────────────────────────────────────

@dataclass
class Route:
    """A sync route: source → [transforms] → sink."""
    name: str
    source_uri: str
    transforms: list[Transform] = field(default_factory=list)
    sink: Sink = field(default_factory=lambda: FileSink())
    ctx: dict = field(default_factory=dict)

ROUTES: dict[str, Route] = {
    "nexus-readme": Route(
        name="nexus-readme",
        source_uri="https://docs.ssccs.org/nexus/index.llms.md",
        transforms=[
            StripFrontmatter(),
            PrependHeader("<!-- synced from SSCCS docs -- do not edit directly -->"),
            ImagePathRewrite(),
        ],
        sink=FileSink("README.md"),
        ctx={"base_url": "https://docs.ssccs.org/projects/nexus/"},
    ),
}

def register_route(name: str, route: Route) -> None:
    ROUTES[name] = route

# ── CLI ──────────────────────────────────────────────────────────────────

def list_routes() -> None:
    print("Available sync routes:")
    for name, route in ROUTES.items():
        src = route.source_uri
        transforms = ", ".join(t.name for t in route.transforms) or "none"
        print(f"  {name:<20} {src}")
        print(f"  {'':20} transforms: {transforms}")
        print()

def run_route(name: str) -> None:
    route = ROUTES.get(name)
    if not route:
        print(f"[error] Unknown route: {name}", file=sys.stderr)
        sys.exit(1)

    print(f"[route] {route.name}")
    print(f"  source: {route.source_uri}")

    transport, resolved = resolve_transport(route.source_uri)
    data = transport.fetch(resolved)
    print(f"  fetch: {len(data)} bytes")

    ctx = dict(route.ctx)
    for t in route.transforms:
        data = t.apply(data, ctx)
        print(f"  transform: {t.name} ({len(data)} bytes)")

    route.sink.write(data, ctx)

def main():
    parser = argparse.ArgumentParser(description="SSCCS universal sync router")
    parser.add_argument("--route", default="nexus-readme",
                        help="Route name (default: nexus-readme)")
    parser.add_argument("--list-routes", action="store_true",
                        help="List available routes and exit")
    parser.add_argument("--source", help="Override source URI for the route")
    parser.add_argument("--output", help="Override output path for the route")
    args = parser.parse_args()

    if args.list_routes:
        list_routes()
        return

    if args.source or args.output:
        # Create an ad-hoc route with overrides
        route = ROUTES.get(args.route)
        if not route:
            print(f"[error] Unknown route: {args.route}", file=sys.stderr)
            sys.exit(1)
        # We can't easily deep-copy transforms, so we modify the route's ctx
        if args.source:
            route.source_uri = args.source
        if args.output:
            route.sink = FileSink(args.output)

    run_route(args.route)

if __name__ == "__main__":
    main()
