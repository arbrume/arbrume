"""Presentation of species parameters: labels, groups, slider ranges and tooltips.

The parameter list itself comes from the species data (see :mod:`.schema`); this table only
says how to show the parameters it knows. A parameter missing here still gets a field, with
a label made from its name, the group "Other" and a slider range guessed from its value.
Slider ranges are soft limits for dragging; any value can be typed, and the generator
checks the domain.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Meta:
    """How one parameter is shown."""

    label: str
    group: str = ""
    soft_min: float | None = None
    soft_max: float | None = None
    description: str = ""


SECTIONS: dict[str, str] = {
    "architecture": "Architecture",
    "growth": "Growth",
    "geometry": "Geometry",
    "foliage": "Foliage",
    "bark": "Bark",
}
"""Top-level tables shown as panels, in order; other tables found in the data follow."""

HIDDEN_TOP_LEVEL = frozenset({"name", "schema_version", "variation"})
"""Top-level keys that are not edited as parameters."""

CURVE_FOCUS = (
    "growth.axis_length",
    "growth.branching_angle",
    "growth.branch_probability",
    "growth.internode_length",
    "foliage.size_along_axis",
)
"""Gradient parameters listed first in their group, since they shape the plant most."""

_LEN = "Lengths and extension"
_BR = "Branching"
_SH = "Shape of an axis"
_FK = "Forks, relays and stems"
_BD = "Bending"

TABLE: dict[str, Meta] = {
    "architecture.branching": Meta("Branching", "", description="Monopodial or sympodial"),
    "architecture.growth_rhythm": Meta("Growth rhythm", ""),
    "architecture.branching_pattern": Meta("Branching pattern", ""),
    "architecture.branch_timing": Meta("Branch timing", ""),
    "architecture.max_order": Meta("Max order", "", 0, 6, "Highest branching order"),
    "architecture.phyllotaxis.leaves_per_node": Meta("Leaves per node", "", 1, 6),
    "architecture.phyllotaxis.divergence_deg": Meta("Divergence (deg)", "", 0, 180),
    "growth.mode": Meta("Growth mode", "", description="Parametric or self-organizing growth"),
    "growth.axis_length": Meta(
        "Axis length", _LEN, 0, 20, "Full-grown axis length over the position on the parent"
    ),
    "growth.shoot_length": Meta("Shoot length", _LEN, 0, 2, "Extension per growth cycle"),
    "growth.internode_length": Meta("Internode length", _LEN, 0.01, 1),
    "growth.length_variation": Meta("Length variation", _LEN, 0, 1),
    "growth.apical_dominance": Meta("Apical dominance", _LEN, 0, 1),
    "growth.parent_length_exponent": Meta("Parent length exponent", _LEN, 0, 1),
    "growth.branch_probability": Meta("Branch probability", _BR, 0, 1),
    "growth.bare_base": Meta("Bare base", _BR, 0, 1),
    "growth.branch_lifespan": Meta("Branch lifespan", _BR, 0, 50),
    "growth.aging_angle": Meta("Aging angle", _BR, 0, 90),
    "growth.unit_branching": Meta("Unit branching", _BR, 0, 1),
    "growth.whorl": Meta("Whorl", _BR, 0, 8),
    "growth.branching_angle": Meta("Branching angle", _BR, 0, 180),
    "growth.branching_angle_variation": Meta("Angle variation", _BR, 0, 45),
    "growth.phyllotaxis_variation": Meta("Phyllotaxis variation", _BR, 0, 45),
    "growth.gravitropic_setpoint": Meta("Gravitropic set-point", _SH, 0, 180),
    "growth.tropism_weight": Meta("Tropism weight", _SH, 0, 0.5),
    "growth.environment_weight": Meta("Environment weight", _SH, 0, 1),
    "growth.curvature": Meta("Curvature", _SH, -90, 90),
    "growth.gnarl": Meta("Gnarl", _SH, 0, 45),
    "growth.direction_variation": Meta("Direction variation", _SH, 0, 30),
    "growth.fork_probability": Meta("Fork probability", _FK, 0, 1),
    "growth.fork_limit": Meta("Fork limit", _FK, 0, 255),
    "growth.fork_angle": Meta("Fork angle", _FK, 0, 90),
    "growth.relay_angle": Meta("Relay angle", _FK, 0, 90),
    "growth.stems": Meta("Stems", _FK, 1, 20),
    "growth.stem_spread": Meta("Stem spread", _FK, 0, 2),
    "growth.stem_angle": Meta("Stem angle", _FK, 0, 60),
    "growth.weight_bending": Meta("Weight bending", _BD, 0, 0.01),
    "geometry.radius_model": Meta("Radius model"),
    "geometry.pipe_exponent": Meta("Pipe exponent", "", 1.5, 3.5),
    "geometry.tip_radius": Meta("Tip radius", "", 0, 0.02),
    "geometry.radius_ratio": Meta("Radius ratio", "", 0, 0.05),
    "geometry.child_radius_exponent": Meta("Child radius exponent", "", 0, 2),
    "geometry.taper": Meta("Taper", "", 0, 1),
    "geometry.flare": Meta("Flare", "Trunk base and collars", 0, 2),
    "geometry.buttress_lobes": Meta("Buttress lobes", "Trunk base and collars", 0, 16),
    "geometry.buttress_depth": Meta("Buttress depth", "Trunk base and collars", 0, 1),
    "geometry.collar_flare": Meta("Collar flare", "Trunk base and collars", 0, 1),
    "geometry.collar_length": Meta("Collar length", "Trunk base and collars", 0, 10),
    "geometry.radial_segments": Meta("Radial segments", "", 3, 64),
    "foliage.enabled": Meta("Leaves", "Leaf sites"),
    "foliage.retention_cycles": Meta("Retention cycles", "Leaf sites", 0, 10),
    "foliage.density": Meta("Density", "Leaf sites", 0, 1),
    "foliage.per_node": Meta("Per node", "Leaf sites", 0, 8),
    "foliage.min_order": Meta("Min order", "Leaf sites", 0, 6),
    "foliage.size_along_axis": Meta("Size along axis", "Leaf sites", 0, 2),
    "foliage.inclination_deg": Meta("Inclination (deg)", "Leaf sites", 0, 180),
    "foliage.length": Meta("Length", "Leaf", 0.005, 1),
    "foliage.width_ratio": Meta("Width ratio", "Leaf", 0.02, 2),
    "foliage.color": Meta("Colour", "Leaf"),
    "foliage.roughness": Meta("Roughness", "Leaf", 0, 1),
    "foliage.geometry": Meta("Geometry", "Leaf"),
    "foliage.shape": Meta("Shape", "Leaf"),
    "foliage.segments": Meta("Segments", "Leaf", 1, 16),
    "foliage.fold_deg": Meta("Fold (deg)", "Leaf", 0, 90),
    "foliage.curvature_deg": Meta("Curvature (deg)", "Leaf", -90, 90),
    "foliage.petiole_length": Meta("Petiole length", "Leaf", 0, 1),
    "foliage.leaflet_pairs": Meta("Leaflet pairs", "Leaf", 0, 32),
    "foliage.leaflet_angle_deg": Meta("Leaflet angle (deg)", "Leaf", 0, 90),
    "foliage.size_variation": Meta("Size variation", "Variation", 0, 0.99),
    "foliage.orientation_variation_deg": Meta("Orientation variation", "Variation", 0, 45),
    "foliage.hue_variation": Meta("Hue variation", "Variation", 0, 1),
    "foliage.brightness_variation": Meta("Brightness variation", "Variation", 0, 1),
    "foliage.twig.leaves": Meta("Leaves", "", 0, 64),
    "foliage.twig.length": Meta("Length", "", 0, 1),
    "foliage.twig.leaves_per_node": Meta("Leaves per node", "", 1, 6),
    "foliage.twig.divergence_deg": Meta("Divergence (deg)", "", 0, 180),
    "foliage.twig.inclination_deg": Meta("Inclination (deg)", "", 0, 180),
    "foliage.twig.bend_deg": Meta("Bend (deg)", "", -90, 90),
    "foliage.twig.stem_radius": Meta("Stem radius", "", 0, 0.01),
    "foliage.twig.fins": Meta("Fins", "", 0, 8),
    "foliage.twig.fin_width": Meta("Fin width", "", 0, 0.1),
    "foliage.twig.fin_shade": Meta("Fin shade", "", 0, 1),
    "foliage.twig.variants": Meta("Variants", "", 1, 16),
    "bark.color": Meta("Colour"),
    "bark.young_color": Meta("Young colour"),
    "bark.young_radius": Meta("Young radius", "", 0, 0.2),
    "bark.color_variation": Meta("Colour variation", "", 0, 1),
    "bark.roughness": Meta("Roughness", "", 0, 1),
    "bark.texture_size": Meta("Texture size", "", 0.05, 2),
}


def label_from_key(key: str) -> str:
    """A readable label for a field name: ``"stem_spread"`` gives ``"Stem spread"``."""
    text = key.replace("_", " ").strip()
    for unit in (" deg",):
        if text.endswith(unit):
            text = text[: -len(unit)] + " (deg)"
    return text[:1].upper() + text[1:]


def guessed_range(key: str, value: float, integer: bool) -> tuple[float, float]:
    """Slider range for a parameter missing from :data:`TABLE`."""
    if key.endswith("_deg") or "angle" in key:
        return 0.0, 180.0
    if any(word in key for word in ("probability", "ratio", "weight", "variation", "fraction")):
        return 0.0, 1.0
    top = max(10.0 if integer else 1.0, 2.0 * abs(value))
    low = min(0.0, 2.0 * value)
    return low, float(round(top)) if integer else top


def meta_for(path: str, value: float = 0.0, integer: bool = False) -> Meta:
    """How to show ``path``: its entry in :data:`TABLE`, or a guess from its name."""
    known = TABLE.get(path)
    if known is not None:
        return known
    key = path.rsplit(".", 1)[-1]
    low, high = guessed_range(key, value, integer)
    return Meta(label_from_key(key), "Other", low, high)
