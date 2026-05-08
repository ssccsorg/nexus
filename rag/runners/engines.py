"""Engine registry — auto-discover all available engines."""

from lightrag.runner import LightRAGEngine
from edgequake.runner import EdgeQuakeEngine
from graphiti.runner import GraphitiEngine

ALL: dict[str, type] = {
    "lightrag": LightRAGEngine,
    "edgequake": EdgeQuakeEngine,
    "graphiti": GraphitiEngine,
}
