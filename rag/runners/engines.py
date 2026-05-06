"""Engine registry — auto-discover all available engines."""

from lightrag.runner import LightRAGEngine
from edgequake.runner import EdgeQuakeEngine

ALL: dict[str, type] = {
    LightRAGEngine.name: LightRAGEngine,
    EdgeQuakeEngine.name: EdgeQuakeEngine,
}
