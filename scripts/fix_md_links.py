#!/usr/bin/env python3
"""
Fix relative .md links in Starlight docs.

Starlight does NOT convert relative `file.md` hrefs to proper URLs —
the browser resolves them relative to the trailing-slash page URL, causing 404s.

Strategy:
  1. Walk every .md file under docs/
  2. Find every link whose target ends in .md and is NOT absolute (http/https)
  3. Resolve the relative path against the source file's location
  4. If the target file EXISTS → replace with absolute /docs/… URL
  5. If the target does NOT exist → report it (still 404, needs manual fix)
"""

import re
import sys
from pathlib import Path

DOCS_ROOT = Path(__file__).parent.parent / "docs"
DOCS_URL_PREFIX = "/docs/"

# Regex: matches [text](relative-path.md) or [text](./relative-path.md)
# Does NOT match [text](http://…), [text](/absolute/…), [text](#anchor)
LINK_RE = re.compile(
    r'\[(?P<text>[^\]]*)\]\((?P<href>[^)#]+\.md(?:[^)]*)?)\)',
    re.IGNORECASE
)


def file_to_url(source_md: Path, rel_target: str) -> str | None:
    """
    Resolve rel_target (like '../deep-dives/foo.md') relative to source_md.
    Return absolute /docs/… URL if target file exists, else None.
    """
    # Strip anchors from href
    anchor = ""
    if "#" in rel_target:
        rel_target, anchor = rel_target.split("#", 1)
        anchor = "#" + anchor

    # Only process relative links (not http, not /absolute)
    if rel_target.startswith("http") or rel_target.startswith("/"):
        return None  # skip – already absolute or external

    # Resolve the path relative to the source file's directory
    target_path = (source_md.parent / rel_target).resolve()

    # Verify it's inside our docs root
    try:
        target_path.relative_to(DOCS_ROOT.resolve())
    except ValueError:
        return None  # outside docs root – skip

    # Check file exists on disk
    if not target_path.exists():
        return None  # file missing – can't auto-fix

    # Build URL: strip DOCS_ROOT prefix and .md suffix
    rel = target_path.relative_to(DOCS_ROOT.resolve())
    parts = list(rel.parts)
    # Remove .md extension from the last part
    last = parts[-1]
    if last.lower() == "index.md":
        parts = parts[:-1]  # index → directory URL
    else:
        parts[-1] = re.sub(r'\.md$', '', last, flags=re.IGNORECASE)

    url = DOCS_URL_PREFIX + "/".join(parts) + "/" + anchor
    return url


def process_file(md_file: Path) -> tuple[int, list[str]]:
    """Process one .md file. Return (num_fixes, missing_targets)."""
    original = md_file.read_text(encoding="utf-8")
    fixed = original
    missing = []
    fixes = 0

    for m in LINK_RE.finditer(original):
        href = m.group("href")
        text = m.group("text")

        # Skip already-absolute and external links
        if href.startswith("http") or href.startswith("/") or href.startswith("#"):
            continue

        # Strip anchor for resolution
        bare_href = href.split("#")[0]
        anchor = ("#" + href.split("#", 1)[1]) if "#" in href else ""

        # Resolve
        new_url = file_to_url(md_file, href)
        if new_url is not None:
            # Replace old href with new absolute URL
            old_md = f"[{text}]({href})"
            new_md = f"[{text}]({new_url})"
            if old_md in fixed:
                fixed = fixed.replace(old_md, new_md, 1)
                fixes += 1
        else:
            # Try to resolve the path even if file is missing, for reporting
            target = (md_file.parent / bare_href).resolve()
            try:
                target.relative_to(DOCS_ROOT.resolve())
                inside = True
            except ValueError:
                inside = False

            if not (href.startswith("http") or href.startswith("/")):
                missing.append(f"  MISSING: [{text}]({href}) → {target}")

    if fixed != original:
        md_file.write_text(fixed, encoding="utf-8")

    return fixes, missing


def main():
    total_fixes = 0
    all_missing = []

    md_files = sorted(DOCS_ROOT.rglob("*.md"))
    print(f"Scanning {len(md_files)} markdown files under {DOCS_ROOT}\n")

    for md_file in md_files:
        fixes, missing = process_file(md_file)
        rel = md_file.relative_to(DOCS_ROOT)
        if fixes or missing:
            print(f"  [{rel}]  fixed={fixes}  missing={len(missing)}")
            for m in missing:
                print(m)
        total_fixes += fixes
        all_missing.extend((str(rel), m) for m in missing)

    print(f"\n{'='*60}")
    print(f"Total links fixed : {total_fixes}")
    print(f"Missing targets   : {len(all_missing)}")
    if all_missing:
        print("\n⚠  Links that could NOT be auto-fixed (target file missing):")
        for fname, msg in all_missing:
            print(f"  {fname}: {msg}")
    print()

    return 0 if not all_missing else 1


if __name__ == "__main__":
    sys.exit(main())
