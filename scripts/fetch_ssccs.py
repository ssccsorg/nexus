#!/usr/bin/env python3
"""
fetch_ssccs.py — Universal sync router for SSCCS artifacts.

Downloads happen in GitHub Actions via wrangler/curl. This script only
transforms and writes. The workflow passes a local file path.

Usage:
  python fetch_ssccs.py                          # route defaults → README.md
  python fetch_ssccs.py --route custom
  python fetch_ssccs.py --input /tmp/doc.md --output README.md
  python fetch_ssccs.py --list-routes

Design:
  Route = source path → [transforms] → sink path
  Each transform is a pure function (bytes → bytes).
  New transforms register via Transform subclasses.
  New routes register via ROUTES dict.
"""

import argparse
import os
import re
import sys
from abc import ABC, abstractmethod
from dataclasses import dataclass, field

# ── Transforms ────────────────────────────────────────────────────────────

class Transform(ABC):
    """Pure function: bytes → bytes."""
    name: str = "unnamed"
    @abstractmethod
    def apply(self, data: bytes, ctx: dict) -> bytes: ...


class ImagePathRewrite(Transform):
    """Rewrite relative image paths to absolute URLs.

    Base URL is derived from the source document's own directory:
    if source is /tmp/projects/nexus/index.llms.md, base becomes
    https://docs.ssccs.org/projects/nexus/
    """

    name = "image-path-rewrite"
    IMAGE_EXTS = (".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp")

    def apply(self, data: bytes, ctx: dict) -> bytes:
        base = ctx["source_dir"]
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


class StripFrontmatter(Transform):
    """Remove YAML frontmatter (--- ... ---)."""
    name = "strip-frontmatter"

    def apply(self, data: bytes, ctx: dict) -> bytes:
        text = data.decode("utf-8")
        stripped = re.sub(r'^---\n.*?\n---\n', '', text, count=1, flags=re.DOTALL)
        return stripped.encode("utf-8")


class PrependHeader(Transform):
    """Prepend a comment header."""
    name = "prepend-header"

    def __init__(self, header: str = ""):
        self.header = header

    def apply(self, data: bytes, ctx: dict) -> bytes:
        hdr = ctx.get("header", self.header)
        return (hdr + "\n\n").encode("utf-8") + data if hdr else data


# ── Routes ────────────────────────────────────────────────────────────────

@dataclass
class Route:
    """A sync route: local relative path → [transforms] → local output path."""
    name: str
    input_rel: str                               # relative path under sync root
    transforms: list[Transform] = field(default_factory=list)
    sink: str = "README.md"
    source_dir: str = "https://docs.ssccs.org/projects/nexus/"  # URL prefix for images

ROUTES: dict[str, Route] = {
    "nexus-readme": Route(
        name="nexus-readme",
        input_rel="docs/_llm/projects/nexus/index.llms.md",
        transforms=[
            StripFrontmatter(),
            PrependHeader("<!-- synced from SSCCS docs -- do not edit directly -->"),
            ImagePathRewrite(),
        ],
        sink="README.md",
    ),
}


def register_route(name: str, route: Route) -> None:
    ROUTES[name] = route


def list_routes() -> None:
    print("Available routes:")
    for name, route in ROUTES.items():
        transforms = ", ".join(t.name for t in route.transforms) or "none"
        print(f"  {name:<20} input: {route.input_rel}")
        print(f"  {'':20} transforms: {transforms}")
        print(f"  {'':20} sink: {route.sink}")
        print()


def run_route(route: Route, sync_root: str) -> None:
    input_path = sync_root.rstrip("/") + "/" + route.input_rel.lstrip("/")
    if not os.path.isfile(input_path):
        print(f"[error] Route '{route.name}': file not found at {input_path}", file=sys.stderr)
        print(f"  Run: aws s3 sync s3://ssccs-nexus-af/ssccs {sync_root} --endpoint-url <host>", file=sys.stderr)
        sys.exit(1)
    with open(input_path, "rb") as f:
        data = f.read()
    print(f"[read] {input_path} ({len(data)} bytes)")

    ctx = {"source_dir": route.source_dir}
    for t in route.transforms:
        data = t.apply(data, ctx)
        print(f"  [{t.name}] {len(data)} bytes")

    with open(route.sink, "wb") as f:
        f.write(data)
    print(f"[write] {route.sink} ({len(data)} bytes)")


# ── CLI ───────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(description="SSCCS artifact sync router")
    parser.add_argument("--route", default="nexus-readme",
                        help="Route name (default: nexus-readme)")
    parser.add_argument("--sync-root", default="/tmp",
                        help="Local directory where aws s3 sync downloaded ssccs/ (default: /tmp)")
    parser.add_argument("--output", default=None,
                        help="Output file path (overrides route sink)")
    parser.add_argument("--list-routes", action="store_true",
                        help="List available routes")
    args = parser.parse_args()

    if args.list_routes:
        list_routes()
        return

    route = ROUTES.get(args.route)
    if not route:
        print(f"[error] Unknown route: {args.route}", file=sys.stderr)
        sys.exit(1)

    if args.output:
        route.sink = args.output

    run_route(route, args.sync_root)


if __name__ == "__main__":
    main()
