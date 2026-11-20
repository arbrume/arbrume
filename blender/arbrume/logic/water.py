"""The water of a plant and of a forest, without bpy.

A plant reads its water surface from its species (``SPECIES``), from a height above the
plant (``HEIGHT``) or from the top of an object such as a plane (``OBJECT``); a steady
current is given in Blender's X and Y. Blender is Z up and Arbrume Y up: a vector
``(x, y, z)`` of Blender is ``(x, z, -y)`` in Arbrume, so the current ``(cx, cy)``
becomes ``[cx, 0, -cy]``. Land species ignore the water.
"""

from typing import Any

MODES = (
    ("SPECIES", "Species", "Water as deep as the species says (aquatic species only)"),
    ("HEIGHT", "Height", "Water surface at a height above the plant"),
    ("OBJECT", "Object", "Water surface at the top of an object, such as a plane"),
)


def current_vector(current_xy: tuple[float, float]) -> list[float]:
    """A current in Blender's X and Y as an Arbrume vector."""
    return [float(current_xy[0]), 0.0, -float(current_xy[1])]


def surface_height(
    mode: str, height: float, object_top: float | None, plant_z: float
) -> float | None:
    """Height of the water surface above the base of the plant, ``None`` for the depth
    of the species (or an ``OBJECT`` mode without an object)."""
    if mode == "HEIGHT":
        return float(height)
    if mode == "OBJECT" and object_top is not None:
        return float(object_top) - float(plant_z)
    return None


def water_argument(
    mode: str,
    height: float,
    object_top: float | None,
    plant_z: float,
    current_xy: tuple[float, float],
) -> dict[str, Any] | None:
    """The ``water`` keyword of ``arbrume.generate`` for a plant, ``None`` for the water
    of the species, still."""
    surface = surface_height(mode, height, object_top, plant_z)
    moving = any(abs(float(c)) > 0.0 for c in current_xy)
    if surface is None and not moving:
        return None
    water: dict[str, Any] = {"current": current_vector(current_xy)}
    if surface is not None:
        water["surface"] = surface
    return water


def water_level(mode: str, height: float, object_top: float | None) -> float | None:
    """The ``placement.water_level`` of a forest: a height in world Z, the top of an
    object, or ``None`` for dry land."""
    if mode == "HEIGHT":
        return float(height)
    if mode == "OBJECT" and object_top is not None:
        return float(object_top)
    return None
