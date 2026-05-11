"""Base engine interface. All engines must inherit from AbstractEngine."""

from abc import ABC, abstractmethod
from dataclasses import dataclass


@dataclass
class EngineInfo:
    name: str
    entries: dict[str, str]


class AbstractEngine(ABC):
    @property
    @abstractmethod
    def name(self) -> str: ...

    @property
    @abstractmethod
    def tunnel_config(self) -> str: ...

    @abstractmethod
    def check(self) -> bool: ...

    @abstractmethod
    def start(self, refresh: bool = False) -> None: ...

    @abstractmethod
    def stop(self) -> None: ...

    @abstractmethod
    def health_check(self, timeout_sec: int) -> bool: ...

    @abstractmethod
    def info(self) -> EngineInfo: ...
