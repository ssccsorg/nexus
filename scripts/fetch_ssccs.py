#!/usr/bin/env python3
"""
fetch_ssccs.py — Universal sync agent for SSCCS artifacts.

Fetches a rendered Markdown file from R2 (or any URL), converts relative
image paths to absolute URLs, and writes the result to a local path.

Usage:
  ./fetch_ssccs.py                          # defaults: nexus/index.llms.md → README.md
  ./fetch_ssccs.py --source docs/index.llms.md --output README.md
  ./fetch_ssccs.py --source https://docs.ssccs.org/index.llms.md

Future: extend --source to accept R2 bucket paths, IPFS CIDs, or local files.
The abstraction: one source→destination sync with image path rewriting.
"""

import argparse
import os
import re
import sys
from urllib.request import urlopen, Request

# Defaults
DEFAULT_SOURCE = "nexus/index.llms.md"
DEFAULT_BASE = "https://docs.ssccs.org/projects/nexus/"
DEFAULT_OUTPUT = "README.md"

IMAGE_EXTENSIONS = (".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp")


def is_image_path(path: str) -> bool:
    return any(path.lower().endswith(ext) for ext in IMAGE_EXTENSIONS)


def convert_image_paths(markdown: str, base_url: str) -> str:
    """Rewrite relative image paths in Markdown to absolute URLs."""

    def replace(match):
        alt_text = match.group(1)
        img_path = match.group(2)
        if img_path.startswith("http") or img_path.startswith("//"):
            return match.group(0)
        if not is_image_path(img_path):
            return match.group(0)
        absolute = base_url.rstrip("/") + "/" + img_path.lstrip("/")
        return f"![{alt_text}]({absolute})"

    return re.sub(r'!\[([^\]]*)\]\(([^)]+)\)', replace, markdown)


def fetch(url: str) -> str:
    headers = {"User-Agent": "ssccs-fetch/0.1"}
    req = Request(url, headers=headers)
    with urlopen(req, timeout=30) as resp:
        return resp.read().decode("utf-8")


def resolve_url(source: str) -> str:
    """Resolve source to a fetchable URL.
    
    Current: source is a path relative to docs.ssccs.org.
    Future: R2 bucket paths, IPFS CIDs, local files.
    """
    if source.startswith("http://") or source.startswith("https://"):
        return source
    return f"https://docs.ssccs.org/{source.lstrip('/')}"


def main():
    parser = argparse.ArgumentParser(description="Fetch and sync SSCCS Markdown artifacts")
    parser.add_argument("--source", default=DEFAULT_SOURCE,
                        help=f"Source path or URL (default: {DEFAULT_SOURCE})")
    parser.add_argument("--base", default=DEFAULT_BASE,
                        help=f"Base URL for relative image paths (default: {DEFAULT_BASE})")
    parser.add_argument("--output", default=DEFAULT_OUTPUT,
                        help=f"Output file path (default: {DEFAULT_OUTPUT})")
    args = parser.parse_args()

    url = resolve_url(args.source)
    print(f"[fetch] {url} → {args.output}")

    try:
        content = fetch(url)
    except Exception as e:
        print(f"[fetch] Error: {e}", file=sys.stderr)
        sys.exit(1)

    converted = convert_image_paths(content, args.base)

    with open(args.output, "w", encoding="utf-8") as f:
        f.write(converted)

    print(f"[fetch] Written {args.output} ({len(converted)} bytes)")


if __name__ == "__main__":
    main()
