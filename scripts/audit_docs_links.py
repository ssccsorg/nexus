#!/usr/bin/env python3
"""
EdgeQuake Documentation Link Auditor
Crawls https://edgequake.com/docs/ and checks every link on every page for 404s.
"""

import sys
import time
import re
import csv
from urllib.parse import urljoin, urlparse
from collections import defaultdict, deque
from concurrent.futures import ThreadPoolExecutor, as_completed

try:
    import requests
    from bs4 import BeautifulSoup
except ImportError:
    print("Installing required packages...")
    import subprocess
    subprocess.check_call([sys.executable, "-m", "pip", "install", "requests", "beautifulsoup4", "-q"])
    import requests
    from bs4 import BeautifulSoup

# ── Configuration ────────────────────────────────────────────────────────────
BASE_URL    = "https://edgequake.com"
START_URL   = "https://edgequake.com/docs/"
CHECK_EXTERNAL = False   # Set True to also check external links
MAX_WORKERS = 8
REQUEST_TIMEOUT = 15
CRAWL_DELAY = 0.1        # seconds between requests to same site

HEADERS = {
    "User-Agent": "EdgeQuake-Link-Auditor/1.0 (internal QA bot)",
    "Accept": "text/html,application/xhtml+xml,*/*",
}

# ── Helpers ───────────────────────────────────────────────────────────────────

def is_internal(url: str) -> bool:
    p = urlparse(url)
    return p.netloc in ("", "edgequake.com", "www.edgequake.com")

def normalise(url: str) -> str:
    """Strip fragment; ensure trailing slash on path-only URLs."""
    p = urlparse(url)
    path = p.path
    # Remove fragment; keep query
    return p._replace(fragment="").geturl()

def get_page(url: str, session: requests.Session):
    try:
        r = session.get(url, timeout=REQUEST_TIMEOUT, allow_redirects=True)
        return r.status_code, r.text, r.url
    except Exception as e:
        return None, None, str(e)

def extract_links(html: str, page_url: str):
    """Return list of (href_absolute, link_text) from <a> tags."""
    soup = BeautifulSoup(html, "html.parser")
    links = []
    for tag in soup.find_all("a", href=True):
        href = tag["href"].strip()
        if not href or href.startswith("mailto:") or href.startswith("javascript:") or href == "#":
            continue
        abs_url = urljoin(page_url, href)
        # Strip fragment
        abs_url = abs_url.split("#")[0]
        if not abs_url:
            continue
        text = tag.get_text(strip=True) or "(no text)"
        links.append((abs_url, text))
    return links

# ── Main Audit ────────────────────────────────────────────────────────────────

def audit():
    session = requests.Session()
    session.headers.update(HEADERS)

    # Pages to crawl (internal docs only)
    to_crawl: deque    = deque([START_URL])
    crawled: set       = set()
    # url → status_code
    link_status: dict  = {}
    # broken: {broken_url: [(source_page, link_text), ...]}
    broken: dict       = defaultdict(list)
    # All pages found
    pages_found: set   = set()
    pages_found.add(START_URL)

    print(f"🔍 Starting crawl from {START_URL}\n")

    def check_url(url: str) -> tuple:
        """Return (url, status_code)."""
        if url in link_status:
            return url, link_status[url]
        try:
            r = session.head(url, timeout=REQUEST_TIMEOUT, allow_redirects=True)
            if r.status_code == 405:   # HEAD not allowed → fall back to GET
                r = session.get(url, timeout=REQUEST_TIMEOUT, allow_redirects=True)
            return url, r.status_code
        except Exception as e:
            return url, f"ERR:{e}"

    page_count = 0

    while to_crawl:
        page_url = to_crawl.popleft()
        if page_url in crawled:
            continue
        crawled.add(page_url)
        page_count += 1

        if page_count % 10 == 0:
            print(f"  … crawled {page_count} pages, {len(to_crawl)} in queue")

        status, html, final_url = get_page(page_url, session)
        link_status[page_url] = status

        if status is None:
            print(f"  ✗ ERROR fetching {page_url}: {final_url}")
            continue

        if status != 200:
            print(f"  ✗ HTTP {status}  {page_url}")
            continue

        time.sleep(CRAWL_DELAY)

        links = extract_links(html, final_url)

        # Check each link
        urls_to_check = []
        for abs_url, text in links:
            internal = is_internal(abs_url)
            if not internal and not CHECK_EXTERNAL:
                continue
            urls_to_check.append((abs_url, text))

        # Batch-check with thread pool
        unchecked = [(u, t) for u, t in urls_to_check if u not in link_status]
        if unchecked:
            with ThreadPoolExecutor(max_workers=MAX_WORKERS) as pool:
                futures = {pool.submit(check_url, u): (u, t) for u, t in unchecked}
                for fut in as_completed(futures):
                    u, code = fut.result()
                    link_status[u] = code

        # Record broken links and queue new internal pages to crawl
        for abs_url, text in urls_to_check:
            code = link_status.get(abs_url)
            if code == 404 or (isinstance(code, str) and code.startswith("ERR")):
                broken[abs_url].append((page_url, text))
                print(f"  ❌ {code}  {abs_url}  (on: {page_url})")
            # Queue new internal pages
            parsed = urlparse(abs_url)
            if is_internal(abs_url) and abs_url not in crawled and abs_url not in pages_found:
                # Only crawl docs pages (and homepage)
                if parsed.path.startswith("/docs") or parsed.path in ("/", ""):
                    pages_found.add(abs_url)
                    to_crawl.append(abs_url)

    # ── Report ─────────────────────────────────────────────────────────────────
    print(f"\n{'='*70}")
    print(f"  AUDIT COMPLETE: {page_count} pages crawled")
    print(f"{'='*70}\n")

    if not broken:
        print("✅  No broken links found! All links return 200 OK.\n")
        return 0

    print(f"❌  Found {len(broken)} broken URL(s):\n")
    for i, (url, sources) in enumerate(sorted(broken.items()), 1):
        code = link_status.get(url, "?")
        print(f"  [{i}] HTTP {code}  →  {url}")
        for src, txt in sources:
            print(f'        \u2190 "{txt}" on {src}')
        print()

    # Save CSV report
    report_path = "/tmp/edgequake_broken_links.csv"
    with open(report_path, "w", newline="") as f:
        writer = csv.writer(f)
        writer.writerow(["broken_url", "http_code", "source_page", "link_text"])
        for url, sources in sorted(broken.items()):
            code = link_status.get(url, "?")
            for src, txt in sources:
                writer.writerow([url, code, src, txt])
    print(f"📄  Full report saved to: {report_path}\n")

    return len(broken)


if __name__ == "__main__":
    n = audit()
    sys.exit(0 if n == 0 else 1)
