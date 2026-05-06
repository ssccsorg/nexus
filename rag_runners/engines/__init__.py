"""Engine registry — auto-discover all available engines."""

from .lightrag import LightRAGEngine
from .edgequake import EdgeQuakeEngine

# Register engines here.  Adding a new engine = one import line.
ALL: dict[str, type] = {
    LightRAGEngine.name: LightRAGEngine,
    EdgeQuakeEngine.name: EdgeQuakeEngine,
}
