#!/usr/bin/env python3
"""Import LightRAG GraphML knowledge graph into Memgraph."""

import json
import os
import sys
import xml.etree.ElementTree as ET
from urllib import request

MEMGRAPH_URL = os.environ.get("MEMGRAPH_URL", "http://localhost:7689")
GRAPHML_PATH = os.environ.get(
    "GRAPHML_PATH",
    os.path.join(
        os.path.dirname(__file__),
        "..", "..", "rag", "lightrag", "data", "default",
        "graph_chunk_entity_relation.graphml",
    ),
)


def cypher(query: str, params: dict | None = None) -> list:
    payload = json.dumps({"query": query, "params": params or {}}).encode()
    req = request.Request(
        f"{MEMGRAPH_URL}/cypher",
        data=payload,
        headers={"Content-Type": "application/json"},
    )
    try:
        with request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read())
            return data.get("results", [])
    except Exception as e:
        print(f"  [ERROR] {e}")
        return []


def main() -> None:
    print("=== LightRAG → Memgraph Importer ===\n")
    print(f"GraphML: {GRAPHML_PATH}")
    print(f"Memgraph: {MEMGRAPH_URL}\n")

    if not os.path.isfile(GRAPHML_PATH):
        print(f"[ERROR] GraphML not found: {GRAPHML_PATH}")
        sys.exit(1)

    # Check Memgraph is reachable
    try:
        req = request.Request(f"{MEMGRAPH_URL}/health")
        with request.urlopen(req, timeout=5) as resp:
            if resp.status != 200:
                print("[ERROR] Memgraph proxy not healthy")
                sys.exit(1)
    except Exception as e:
        print(f"[ERROR] Cannot reach Memgraph: {e}")
        sys.exit(1)

    print("[OK] Memgraph reachable\n")

    # Parse GraphML
    tree = ET.parse(GRAPHML_PATH)
    root = tree.getroot()
    ns = {"g": "http://graphml.graphdrawing.org/xmlns"}
    graph = root.find(".//g:graph", ns)

    nodes = graph.findall("g:node", ns)
    edges = graph.findall("g:edge", ns)
    print(f"Found {len(nodes)} nodes, {len(edges)} edges\n")

    # Clear existing data
    print("Clearing existing graph...")
    cypher("MATCH (n) DETACH DELETE n")
    print("[OK]\n")

    # Import nodes
    print("Importing concepts...")
    concept_count = 0
    for node in nodes:
        nid = node.get("id")
        etype = node.find("g:data[@key='d1']", ns)
        desc = node.find("g:data[@key='d2']", ns)
        src_id = node.find("g:data[@key='d3']", ns)
        fpath = node.find("g:data[@key='d4']", ns)
        created = node.find("g:data[@key='d5']", ns)

        result = cypher(
            """
            CREATE (c:Concept {
                name: $name,
                entity_type: $etype,
                description: $desc,
                source_id: $src_id,
                file_path: $fpath,
                created_at: $created
            })
            RETURN c.name AS name
            """,
            {
                "name": nid,
                "etype": etype.text if etype is not None else "",
                "desc": (desc.text or "")[:500] if desc is not None else "",
                "src_id": src_id.text if src_id is not None else "",
                "fpath": fpath.text if fpath is not None else "",
                "created": int(created.text) if created is not None else 0,
            },
        )
        if result:
            concept_count += 1

    print(f"[OK] {concept_count} concepts imported\n")

    # Import edges
    print("Importing relationships...")
    edge_count = 0
    for edge in edges:
        src = edge.get("source")
        tgt = edge.get("target")
        weight = edge.find("g:data[@key='d7']", ns)
        desc = edge.find("g:data[@key='d8']", ns)
        keywords = edge.find("g:data[@key='d9']", ns)
        src_id = edge.find("g:data[@key='d10']", ns)
        created = edge.find("g:data[@key='d12']", ns)

        # Use first keyword as relationship type, fallback to RELATES_TO
        kw_text = keywords.text if keywords is not None else ""
        rel_type = kw_text.split(",")[0].strip() if kw_text else "RELATES_TO"
        # Sanitize: Cypher relationship types must be uppercase alphanum + underscore
        rel_type = "".join(c if c.isalnum() or c == "_" else "_" for c in rel_type.upper())
        if not rel_type or rel_type[0].isnumeric():
            rel_type = "RELATES_TO"

        result = cypher(
            f"""
            MATCH (a:Concept {{name: $src}})
            MATCH (b:Concept {{name: $tgt}})
            CREATE (a)-[:{rel_type} {{
                weight: $weight,
                description: $desc,
                keywords: $keywords,
                source_id: $src_id,
                created_at: $created
            }}]->(b)
            RETURN a.name AS src, b.name AS tgt
            """,
            {
                "src": src,
                "tgt": tgt,
                "weight": float(weight.text) if weight is not None else 1.0,
                "desc": (desc.text or "")[:500] if desc is not None else "",
                "keywords": kw_text,
                "src_id": src_id.text if src_id is not None else "",
                "created": int(created.text) if created is not None else 0,
            },
        )
        if result:
            edge_count += 1

    print(f"[OK] {edge_count} relationships imported\n")

    # Summary
    result = cypher("MATCH (c:Concept) RETURN count(c) AS total")
    total_nodes = result[0]["total"] if result else 0
    result = cypher("MATCH ()-[r]->() RETURN count(r) AS total")
    total_edges = result[0]["total"] if result else 0

    print("=== Import Complete ===")
    print(f"  Nodes: {total_nodes}")
    print(f"  Edges: {total_edges}")
    print()

    # Test queries that gap-detector will use
    print("Testing gap-detector queries...")
    for name, query in [
        ("Orphaned concepts", "MATCH (c:Concept) OPTIONAL MATCH (c)-[r]-() WITH c, count(r) AS rc WHERE rc = 0 RETURN count(c) AS n"),
        ("Relationships", "MATCH (a:Concept)-[r]->(b:Concept) RETURN count(r) AS n"),
    ]:
        r = cypher(query)
        print(f"  {name}: {r[0]['n'] if r else '?'}")


if __name__ == "__main__":
    main()
