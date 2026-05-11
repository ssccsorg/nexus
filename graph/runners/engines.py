"""Engine registry — auto-discover all available graph engines."""

from memgraph.runner import MemgraphEngine

ALL: dict[str, type] = {
    "memgraph": MemgraphEngine,
}
