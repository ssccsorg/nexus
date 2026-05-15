"""Engine registry — all external Blackboard implementations."""

from lightrag.runner import LightRAGEngine
from edgequake.runner import EdgeQuakeEngine
from graphiti.runner import GraphitiEngine
from memgraph.runner import MemgraphEngine

ALL: dict[str, type] = {
    "lightrag": LightRAGEngine,
    "edgequake": EdgeQuakeEngine,
    "graphiti": GraphitiEngine,
    "memgraph": MemgraphEngine,
}
