"""Engine registry — auto-discover all available engines."""

from lightrag.runner import LightRAGEngine
from edgequake.runner import EdgeQuakeEngine

ALL: dict[str, type] = {
    "lightrag": LightRAGEngine,
    "edgequake": EdgeQuakeEngine,
}
