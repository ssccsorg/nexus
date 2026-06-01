"""
Base engine interface. All engines must inherit from AbstractEngine.
"""

from abc import ABC, abstractmethod
from dataclasses import dataclass


@dataclass
class EngineInfo:
    """Output displayed when an engine is ready."""

    name: str
    entries: dict[str, str]  # label -> value


class AbstractEngine(ABC):
    """Contract every RAG engine must fulfill."""

    @property
    @abstractmethod
    def name(self) -> str:
        """Unique engine identifier (e.g. 'lightrag', 'edgequake')."""

    @property
    @abstractmethod
    def tunnel_config(self) -> str:
        """Path to the Cloudflare tunnel config YAML for this engine."""

    @abstractmethod
    def check(self) -> bool:
        """Verify prerequisites. Return False if engine cannot run."""

    @abstractmethod
    def start(self, refresh: bool = False) -> None:
        """Launch the engine (blocking calls should be dispatched to subprocess)."""

    @abstractmethod
    def stop(self) -> None:
        """Stop the engine gracefully."""

    @abstractmethod
    def health_check(self, timeout_sec: int) -> bool:
        """Poll until healthy or timeout. Return True if healthy."""

    @abstractmethod
    def info(self) -> EngineInfo:
        """Return display info shown when the engine is ready."""
