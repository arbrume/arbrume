"""Species sources, cached base species and calls into the ``arbrume`` wheel.

A plant names its species by a *source*: a preset name, or the TOML text of a custom species
file. :class:`SpeciesCache` keeps the full species (defaults included) of each source and of
each source with a set of overrides, so drawing the parameter panels does not call the
generator.
"""

from __future__ import annotations

import hashlib
from collections import OrderedDict
from dataclasses import dataclass
from typing import Any

import arbrume

from . import schema
from .values import apply_overrides, dump_overrides, load_overrides

PREFERRED_PROBE_PRESET = "broadleaf"


@dataclass(frozen=True)
class Source:
    """Where a plant's species comes from: a preset name or the text of a species file."""

    preset: str = ""
    toml: str = ""

    @property
    def is_custom(self) -> bool:
        """Whether the species is a custom file rather than a preset."""
        return bool(self.toml)

    @property
    def key(self) -> str:
        """Short stable key of the source, for caches."""
        if self.toml:
            return "toml:" + hashlib.sha1(self.toml.encode()).hexdigest()[:16]
        return "preset:" + self.preset

    @property
    def argument(self) -> str:
        """The ``species`` argument of ``arbrume.generate``."""
        return self.toml or self.preset


def readable_name(preset: str) -> str:
    """Display name of a preset: ``"oak_like"`` gives ``"Oak-like"``."""
    text = preset.replace("_like", "-like").replace("_", " ")
    return text[:1].upper() + text[1:]


def generate(source: Source, overrides: dict | None = None, **kwargs: Any) -> arbrume.Tree:
    """``arbrume.generate`` for a source; raises ``ValueError`` with the generator message."""
    return arbrume.generate(source.argument, overrides=overrides or None, **kwargs)


def full_species(source: Source, overrides: dict | None = None) -> dict:
    """The complete species of a source after overrides, as the generator reads it."""
    return generate(source, overrides, seed=0, age=0).metadata["species"]


def make_probe(preset: str) -> schema.Probe:
    """A probe that tries an override on ``preset`` and returns the error, if any."""

    def probe(path: str, value: Any) -> str | None:
        try:
            arbrume.generate(preset, seed=0, age=0, overrides={path: value})
        except (ValueError, TypeError) as error:
            return str(error)
        return None

    return probe


def preset_schema() -> list[schema.Section]:
    """Sections and parameters found in every preset of the installed wheel."""
    names = arbrume.presets()
    species = [full_species(Source(preset=name)) for name in names]
    base = PREFERRED_PROBE_PRESET if PREFERRED_PROBE_PRESET in names else names[0]
    return schema.build_schema(species, make_probe(base))


class SpeciesCache:
    """Least-recently-used caches of base and effective species, keyed by plain values."""

    def __init__(self, capacity: int = 64) -> None:
        self.capacity = capacity
        self._base: OrderedDict[str, dict] = OrderedDict()
        self._effective: OrderedDict[tuple[str, str], dict] = OrderedDict()
        self.errors: dict[str, str] = {}

    def _remember(self, cache: OrderedDict, key: Any, value: dict) -> dict:
        cache[key] = value
        cache.move_to_end(key)
        while len(cache) > self.capacity:
            cache.popitem(last=False)
        return value

    def base(self, source: Source) -> dict:
        """Full species of the source without overrides; empty when it does not load."""
        key = source.key
        if key in self._base:
            self._base.move_to_end(key)
            return self._base[key]
        try:
            species = full_species(source)
            self.errors.pop(key, None)
        except (ValueError, TypeError, OSError) as error:
            self.errors[key] = str(error)
            species = {}
        return self._remember(self._base, key, species)

    def effective(self, source: Source, overrides: dict) -> dict:
        """Base species of the source with ``overrides`` applied in Python."""
        key = (source.key, dump_overrides(overrides))
        if key in self._effective:
            self._effective.move_to_end(key)
            return self._effective[key]
        return self._remember(self._effective, key, apply_overrides(self.base(source), overrides))

    def effective_text(self, source: Source, overrides_text: str) -> dict:
        """:meth:`effective` for overrides stored as JSON text, cached by that text."""
        key = (source.key, "text:" + overrides_text)
        if key in self._effective:
            self._effective.move_to_end(key)
            return self._effective[key]
        return self._remember(
            self._effective, key, self.effective(source, load_overrides(overrides_text))
        )

    def clear(self) -> None:
        """Forget everything."""
        self._base.clear()
        self._effective.clear()
        self.errors.clear()
