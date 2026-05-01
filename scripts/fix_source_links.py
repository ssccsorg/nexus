#!/usr/bin/env python3
"""
Fix broken source code links in docs that point to repository files via relative paths.
Converts (../)+edgequake/crates/... relative paths to GitHub blob/tree URLs.
Also fixes the `crates/` relative link in architecture/overview.md.
"""

import re
import sys
from pathlib import Path

GITHUB_BASE = "https://github.com/raphaelmansuy/edgequake"
BRANCH = "edgequake-main"
DOCS_DIR = Path(__file__).parent.parent / "docs"

# Regex: match markdown links like [text](../../edgequake/crates/SUBPATH)
# Captures: link_text, relative_dots (../.. etc), subpath (everything after edgequake/crates/)
PATTERN = re.compile(
    r'\[([^\]]+)\]\((?:\.\./)+edgequake/crates/((?:[^)#\n]+?))(#[^)]+)?\)'
)


def is_dir_path(path: str) -> bool:
    return path.endswith("/")


def make_github_url(subpath: str, anchor: str = "") -> str:
    kind = "tree" if is_dir_path(subpath) else "blob"
    url = f"{GITHUB_BASE}/{kind}/{BRANCH}/edgequake/crates/{subpath}"
    return url + (anchor or "")


def fix_file(path: Path) -> int:
    text = path.read_text(encoding="utf-8")
    replacements = 0

    def replace_match(m: re.Match) -> str:
        nonlocal replacements
        link_text, subpath, anchor = m.group(1), m.group(2), m.group(3) or ""
        new_url = make_github_url(subpath, anchor)
        replacements += 1
        return f"[{link_text}]({new_url})"

    new_text = PATTERN.sub(replace_match, text)

    # Fix the architecture/overview.md `crates/` relative link
    if path.name == "overview.md" and "architecture" in str(path):
        count_before = new_text.count("](crates/)")
        new_text = new_text.replace("](crates/)", "](/docs/architecture/crates/)")
        replacements += new_text.count("](/docs/architecture/crates/)") - (
            new_text.count("](crates/)") - count_before
        )
        if "](/docs/architecture/crates/)" in new_text and "](crates/)" not in new_text:
            # count successful replacements
            pass

    if new_text != text:
        path.write_text(new_text, encoding="utf-8")
        print(f"  Fixed {replacements} links in {path.relative_to(DOCS_DIR.parent)}")
    return replacements


def main():
    total = 0
    for md in sorted(DOCS_DIR.rglob("*.md")):
        count = fix_file(md)
        total += count

    # Also fix the architecture/overview.md crates/ link separately
    overview = DOCS_DIR / "architecture" / "overview.md"
    if overview.exists():
        text = overview.read_text(encoding="utf-8")
        if "](crates/)" in text:
            new_text = text.replace("](crates/)", "](/docs/architecture/crates/)")
            overview.write_text(new_text, encoding="utf-8")
            print(f"  Fixed crates/ link in {overview.relative_to(DOCS_DIR.parent)}")
            total += text.count("](crates/)")

    print(f"\nTotal source code links converted to GitHub URLs: {total}")


if __name__ == "__main__":
    main()
