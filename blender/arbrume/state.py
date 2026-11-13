"""Session state of the extension: caches, the parameter schema and the regeneration guard.

Nothing here is saved in a .blend file; everything a plant needs is in its object
properties. :func:`reset` runs on every file load, so no state leaks from one file to the
next.
"""

from __future__ import annotations

import time
from contextlib import contextmanager
from dataclasses import dataclass, field

from .logic.schema import ParamSpec, Section
from .logic.species import SpeciesCache
from .logic.values import CurvePoints


@dataclass
class PlantStatus:
    """Outcome of the last generation of one plant, shown in the panel."""

    error: str = ""
    generate_ms: float = 0.0
    upload_ms: float = 0.0
    vertices: int = 0
    triangles: int = 0


@dataclass
class CurveBinding:
    """A curve parameter of a plant shown in a curve widget."""

    object_name: str
    path: str
    order: int
    low: float = 0.0
    high: float = 1.0
    last: CurvePoints | None = None
    seen: float = field(default_factory=time.monotonic)


@dataclass
class State:
    """All session state of the extension."""

    cache: SpeciesCache = field(default_factory=SpeciesCache)
    sections: list[Section] = field(default_factory=list)
    specs: dict[str, ParamSpec] = field(default_factory=dict)
    schema_error: str = ""
    presets: list[str] = field(default_factory=list)
    status: dict[str, PlantStatus] = field(default_factory=dict)
    bindings: dict[str, CurveBinding] = field(default_factory=dict)
    busy: set[int] = field(default_factory=set)
    suspended: int = 0


STATE = State()


def reset() -> None:
    """Forget the per-file state; the schema and preset list stay valid."""
    STATE.cache.clear()
    STATE.status.clear()
    STATE.bindings.clear()
    STATE.busy.clear()
    STATE.suspended = 0


def status_of(name: str) -> PlantStatus:
    """Status record of the plant object named ``name``."""
    return STATE.status.setdefault(name, PlantStatus())


@contextmanager
def suspended():
    """Within this block, property changes do not regenerate plants."""
    STATE.suspended += 1
    try:
        yield
    finally:
        STATE.suspended -= 1
