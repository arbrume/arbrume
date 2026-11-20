"""The root system of a plant: the ``roots`` argument of ``arbrume.generate`` and when the
envelope and obstacle volumes act on the plant.

A plant chooses its roots with one setting: ``species`` grows the ``[roots]`` layer of
its species when it has one, ``on`` grows roots in any case (the layer of the species, or
the default heart habit), ``off`` grows none. Obstacles block roots as they block shoots,
so a plant that grows roots reads its volumes whatever its growth mode.
"""

from __future__ import annotations

MODES = (
    ("species", "Species", "Roots when the species has a [roots] layer"),
    ("on", "Grow Roots", "Grow a root system, from the species or the heart habit"),
    ("off", "No Roots", "No root system, whatever the species says"),
)


def roots_argument(mode: str) -> bool | None:
    """The ``roots`` keyword of ``arbrume.generate`` for a mode of :data:`MODES`."""
    return {"on": True, "off": False}.get(mode)


def grows_roots(species: dict, mode: str) -> bool:
    """Whether a plant of the effective ``species`` dict grows roots in ``mode``."""
    return mode == "on" or (mode == "species" and isinstance(species.get("roots"), dict))
