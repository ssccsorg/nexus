"""HTTP-to-Bolt proxy for Memgraph.

Accepts POST /cypher with JSON body { "query": "...", "params": {...} }
and returns query results as JSON.
"""

import os
import json
from http.server import HTTPServer, BaseHTTPRequestHandler
from neo4j import GraphDatabase

MEMGRAPH_HOST = os.environ.get("MEMGRAPH_HOST", "127.0.0.1")
MEMGRAPH_PORT = int(os.environ.get("MEMGRAPH_PORT", "7687"))
PROXY_PORT = int(os.environ.get("PROXY_PORT", "7689"))


class CypherHandler(BaseHTTPRequestHandler):
    driver = GraphDatabase.driver(f"bolt://{MEMGRAPH_HOST}:{MEMGRAPH_PORT}")

    def do_POST(self):
        if self.path != "/cypher":
            self.send_error(404, "Use POST /cypher")
            return

        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        try:
            req = json.loads(body)
        except Exception:
            self.send_error(400, "Invalid JSON")
            return

        query = req.get("query", "")
        params = req.get("params", {})
        if not query.strip():
            self.send_error(400, "Missing 'query'")
            return

        try:
            records, summary, keys = self.driver.execute_query(query, **params)
            results = [dict(r) for r in records]
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({
                "results": results,
            }).encode())
        except Exception as e:
            self.send_error(500, str(e))

    def do_GET(self):
        if self.path == "/health":
            try:
                self.driver.verify_connectivity()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.end_headers()
                self.wfile.write(b'{"status":"healthy"}')
            except Exception:
                self.send_error(503, "Memgraph unreachable")
        else:
            self.send_error(404)

    def log_message(self, format, *args):
        msg = format % args if args else format
        print(f"[memgraph-proxy] {msg}")


if __name__ == "__main__":
    server = HTTPServer(("0.0.0.0", PROXY_PORT), CypherHandler)
    print(f"[memgraph-proxy] Listening on :{PROXY_PORT} → {MEMGRAPH_HOST}:{MEMGRAPH_PORT}")
    server.serve_forever()
