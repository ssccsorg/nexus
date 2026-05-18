#!/usr/bin/env python3
"""
fetch_ssccs.py — SSCCS artifact sync router.

Transforms locally-synced R2 artifacts into repo files.
Each route maps a relative input path under the sync root
through a pipeline of transforms to a local output file.

Usage:
  python fetch_ssccs.py                     # run default route
  python fetch_ssccs.py --all               # run all routes
  python fetch_ssccs.py --list-routes       # show available routes
  python fetch_ssccs.py --route custom

Workflow:
  aws s3 sync s3://ssccs-nexus-af/ssccs /tmp/ssccs ...
  python fetch_ssccs.py --sync-root /tmp/ssccs --all
"""

import argparse
import os
import re
import sys
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass

# ── Transforms ────────────────────────────────────────────────────────────


class Transform(ABC):
    """Pure function: bytes → bytes."""
    name: str = "unnamed"

    @abstractmethod
    def apply(self, data: bytes, ctx: dict) -> bytes:
        ...


class StripFrontmatter(Transform):
    """Remove YAML frontmatter (--- ... ---)."""
    name = "strip-frontmatter"

    def apply(self, data: bytes, ctx: dict) -> bytes:
        stripped = re.sub(r'^---\n.*?\n---\n', '', data.decode("utf-8"),
                          count=1, flags=re.DOTALL)
        return stripped.encode("utf-8")


class StripIntroContent(Transform):
    """Remove content between the first h1 and the first h2.

    Keeps both heading markers but discards any introductory paragraphs
    or prose that sit between the top-level title and the first section
    heading.  If either heading is missing the transform is a no-op.
    """
    name = "strip-intro-content"

    def apply(self, data: bytes, ctx: dict) -> bytes:
        text = data.decode("utf-8")
        # Locate the first h1.
        m_h1 = re.search(r'^#\s+.*', text, re.MULTILINE)
        if not m_h1:
            return data
        h1_end = m_h1.end()
        # Locate the first h2 after the h1.
        m_h2 = re.search(r'^##\s', text[h1_end:], re.MULTILINE)
        if not m_h2:
            return data
        h2_start = h1_end + m_h2.start()
        # Assemble: h1 line + newline + h2 and everything after.
        result = text[:h1_end] + '\n' + text[h2_start:]
        size = len(text) - len(result)
        if size:
            print(f"  [{self.name}] stripped {size} byte(s) of intro content")
        return result.encode("utf-8")


class PrependHeader(Transform):
    """Prepend a comment marker."""
    name = "prepend-header"

    def __init__(self, header: str = ""):
        self.header = header

    def apply(self, data: bytes, ctx: dict) -> bytes:
        hdr = ctx.get("header", self.header)
        return (hdr + "\n\n").encode("utf-8") + data if hdr else data


class ImagePathRewrite(Transform):
    """Rewrite relative image paths to absolute URLs.

    Base URL comes from the route's source_dir (the published location).
    Only rewrites paths with image extensions — leaves absolute URLs alone.
    """
    name = "image-path-rewrite"
    EXTS = (".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp")

    def apply(self, data: bytes, ctx: dict) -> bytes:
        base = ctx["source_dir"]
        text = data.decode("utf-8")
        count = 0

        def replace(m):
            nonlocal count
            alt, path = m.group(1), m.group(2)
            if path.startswith("http") or path.startswith("//"):
                return m.group(0)
            if not any(path.lower().endswith(e) for e in self.EXTS):
                return m.group(0)
            count += 1
            return f"![{alt}]({base.rstrip('/')}/{path.lstrip('/')})"

        result = re.sub(r'!\[([^\]]*)\]\(([^)]+)\)', replace, text)
        if count:
            print(f"  [{self.name}] {count} image(s) rewritten → {base}")
        return result.encode("utf-8")


# ── Routes ────────────────────────────────────────────────────────────────


@dataclass
class Route:
    """Declarative sync definition.

    Attributes:
        name:       unique route identifier
        input_rel:  path relative to sync root (e.g. docs/_llms/projects/nexus/index.llms.md)
        transforms: ordered list of Transform instances
        sink:       local output file path
        source_dir: URL prefix for relative image path rewriting (optional per route)
    """
    name: str
    input_rel: str
    transforms: list
    sink: str = "README.md"
    source_dir: str = ""


ROUTES: dict[str, Route] = {
    "nexus-readme": Route(
        name="nexus-readme",
        input_rel="docs/_llms/projects/nexus/index.llms.md",
        transforms=[
            StripFrontmatter(),
            StripIntroContent(),
            PrependHeader("<!-- synced from SSCCS docs -- do not edit directly -->"),
            ImagePathRewrite(),
        ],
        sink="README.md",
        source_dir="https://docs.ssccs.org/projects/nexus/",
    ),
}


def register_route(name: str, route: Route) -> None:
    """Register an external route (for future plugins)."""
    if name in ROUTES:
        raise ValueError(f"Route '{name}' already registered")
    ROUTES[name] = route


def list_routes() -> None:
    print("Available sync routes:")
    for r in ROUTES.values():
        xf = ", ".join(t.name for t in r.transforms) or "none"
        print(f"  {r.name:<25} {r.input_rel}")
        print(f"  {'':25} transforms: {xf}")
        print(f"  {'':25} sink: {r.sink}")
        print()


def run_route(route: Route, sync_root: str) -> int:
    """Execute one route. Returns diff size (0 = no change)."""
    input_path = f"{sync_root.rstrip('/')}/{route.input_rel.lstrip('/')}"
    if not os.path.isfile(input_path):
        print(f"  [skip] file not found: {input_path}", file=sys.stderr)
        return 0

    with open(input_path, "rb") as f:
        data = f.read()

    ctx = {"source_dir": route.source_dir}
    for t in route.transforms:
        data = t.apply(data, ctx)

    # Read existing output (if any) to detect changes
    existing = b""
    if os.path.isfile(route.sink):
        with open(route.sink, "rb") as f:
            existing = f.read()

    if data == existing:
        print(f"  [sync] {route.sink} unchanged")
        return 0

    with open(route.sink, "wb") as f:
        f.write(data)
    diff_size = abs(len(data) - len(existing))
    print(f"  [sync] {route.sink} updated ({diff_size} byte(s) changed)")
    return diff_size


# ── CLI ───────────────────────────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(
        description="SSCCS artifact sync router")
    parser.add_argument("--route", default="nexus-readme",
                        help="Run a single named route")
    parser.add_argument("--sync-root", default="/tmp/ssccs",
                        help="Local sync root (default: /tmp/ssccs)")
    parser.add_argument("--all", action="store_true",
                        help="Run all registered routes")
    parser.add_argument("--list-routes", action="store_true",
                        help="List available routes")
    args = parser.parse_args()

    if args.list_routes:
        list_routes()
        return

    if args.all:
        routes = list(ROUTES.values())
    else:
        route = ROUTES.get(args.route)
        if not route:
            print(f"[error] unknown route: {args.route}", file=sys.stderr)
            sys.exit(1)
        routes = [route]

    changed = 0
    n_ok = 0
    for r in routes:
        print(f"[route] {r.name}")
        t0 = time.time()
        try:
            d = run_route(r, args.sync_root)
            changed += d
            n_ok += 1
        except Exception as e:
            print(f"  [error] {e}", file=sys.stderr)
        print(f"  [{time.time() - t0:.2f}s]")

    print(f"\nSummary: {n_ok}/{len(routes)} routes, {changed} byte(s) changed")
    if changed == 0:
        print("All files up to date.")


if __name__ == "__main__":
    main()
