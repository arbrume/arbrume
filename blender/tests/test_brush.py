"""Brush strokes without Blender: frames, projection, resampling and the guides text."""

import json
import math

import arbrume
import numpy as np
import pytest
from logic import brush


def plant_matrix(location=(0.0, 0.0, 0.0), angle: float = 0.0, scale: float = 1.0):
    """World matrix of a plant turned by ``angle`` about Z."""
    c, s = math.cos(angle), math.sin(angle)
    matrix = np.eye(4)
    matrix[:3, :3] = np.array([[c, -s, 0.0], [s, c, 0.0], [0.0, 0.0, 1.0]]) * scale
    matrix[:3, 3] = location
    return matrix


def test_world_points_go_to_the_plant_frame_and_back() -> None:
    matrix = plant_matrix((2.0, 1.0, 0.5), math.pi / 2, 2.0)
    world = np.array([[2.0, 1.0, 0.5], [2.0, 3.0, 4.5], [0.0, 1.0, 0.5]])
    plant = brush.world_to_plant(matrix, world)
    assert np.allclose(plant[0], 0.0)
    assert np.allclose(plant[1], [1.0, 2.0, 0.0])
    assert np.allclose(plant[2], [0.0, 0.0, -1.0])
    assert np.allclose(brush.plant_to_world(matrix, plant), world)
    assert brush.plant_scale(matrix) == pytest.approx(2.0)


def test_rays_meet_the_drawing_plane() -> None:
    point, normal = brush.drawing_plane((0.0, 3.0, 0.0), (1.0, 2.0, 0.0))
    assert np.allclose(normal, (0.0, 1.0, 0.0))
    hit = brush.ray_plane((5.0, -10.0, 4.0), (0.0, 1.0, 0.0), point, normal)
    assert np.allclose(hit, (5.0, 2.0, 4.0))
    assert brush.ray_plane((0, 0, 0), (1.0, 0.0, 0.0), point, normal) is None
    assert brush.ray_plane((0, 5, 0), (0.0, 1.0, 0.0), point, normal) is None
    with pytest.raises(ValueError):
        brush.drawing_plane((0.0, 0.0, 0.0), (0.0, 0.0, 0.0))


def test_strokes_are_thinned_and_resampled() -> None:
    points: list = []
    for x in (0.0, 0.01, 0.2, 0.25, 0.5):
        if brush.should_append(points, (x, 0.0, 0.0), 0.1):
            points.append((x, 0.0, 0.0))
    assert [p[0] for p in points] == [0.0, 0.2, 0.5]
    line = brush.resample([(0, 0, 0), (1, 0, 0), (1, 1, 0)], 0.25)
    assert len(line) == 9
    steps = np.linalg.norm(np.diff(line, axis=0), axis=1)
    assert np.allclose(steps, 0.25)
    assert np.allclose(line[-1], (1, 1, 0))
    assert len(brush.resample([(0, 0, 0), (100, 0, 0)], 0.01)) == brush.MAX_POINTS
    assert len(brush.resample([(1, 2, 3)], 0.1)) == 1
    assert len(brush.resample([(1, 2, 3), (1, 2, 3)], 0.1)) == 1


def test_erasing_through_sweeps_the_stroke_along_the_view() -> None:
    lines = brush.swept([(0, 0, 0), (1, 0, 0)], (0, 2, 0), 5.0, 0.5)
    assert len(lines) == 3
    assert np.allclose(lines[1], [(0.5, -5, 0), (0.5, 5, 0)])


def test_guides_text_round_trips_with_cycles_and_freezes() -> None:
    guides = brush.load("")
    assert brush.dump(guides) == ""
    crown = brush.make_stroke("CROWN", [(0.123456, 1, 2)], 0.5, {"strength": 2, "follow": 1})
    assert crown == {
        "kind": "crown",
        "points": [[0.1235, 1.0, 2.0]],
        "radius": 0.5,
        "strength": 2.0,
    }
    guides = brush.with_stroke(guides, crown)
    guides = brush.with_freeze(guides, 12)
    assert brush.with_freeze(guides, 10) == guides
    limb = brush.make_stroke("LIMB", [(0, 0, 0), (0, 1, 0)], 0.2, {"follow": 0.8})
    guides = brush.with_stroke(guides, limb)
    assert guides["strokes"][1]["cycle"] == 12 and "cycle" not in guides["strokes"][0]
    text = brush.dump(guides)
    assert brush.load(text) == json.loads(text) == guides
    assert brush.counts(guides) == {"crown": 1, "limb": 1, "erase": 0}
    thawed, age = brush.without_last_freeze(guides)
    assert age == 12 and thawed == {"strokes": [crown], "freezes": []}
    with pytest.raises(ValueError):
        brush.load("[1, 2]")


def test_stored_strokes_are_valid_guides_for_the_generator() -> None:
    guides = brush.with_stroke(
        brush.load(""),
        brush.make_stroke("LIMB", [(0, 2, 0), (1, 3, 0), (2, 3.5, 0)], 0.4, {"follow": 1}),
    )
    guides = brush.with_stroke(guides, brush.make_stroke("ERASE", [(-2, 4, 0)], 1.0, {}))
    tree = arbrume.generate("open_grown_broadleaf", seed=1, age=10, quality=0.2, guides=guides)
    assert tree.guides["strokes"][0]["claimed"]


def species(mode: str, height: float, reach: float, stems: int = 1, climbing=None) -> dict:
    """A species dict with the fields the habit features read."""
    data = {
        "architecture": {"max_order": 4},
        "growth": {
            "mode": mode,
            "stems": stems,
            "axis_length": {"by_order": [height, reach, 1.0]},
        },
    }
    if climbing:
        data["climbing"] = climbing
    return data


def test_the_nearest_self_organizing_preset_has_the_closest_habit() -> None:
    presets = {
        "tall": species("self_organizing", 15.0, 9.0),
        "wide": species("self_organizing", 5.0, 11.0),
        "bush": species("self_organizing", 2.0, 2.0, stems=6),
        "ivy": species("self_organizing", 15.0, 9.0, climbing={"attachment": "adhesive"}),
        "lsys": species("parametric", 15.0, 9.0),
    }
    assert brush.nearest_self_organizing(species("parametric", 16.0, 8.0), presets) == "tall"
    assert brush.nearest_self_organizing(species("parametric", 4.0, 12.0), presets) == "wide"
    shrub = species("parametric", 2.5, 1.5, stems=5)
    assert brush.nearest_self_organizing(shrub, presets) == "bush"
    assert brush.nearest_self_organizing(shrub, {"lsys": presets["lsys"]}) is None


def test_the_shipped_presets_have_a_nearest_self_organizing_one() -> None:
    names = arbrume.presets()
    full = {name: arbrume.generate(name, seed=0, age=0).metadata["species"] for name in names}
    for name, data in full.items():
        nearest = brush.nearest_self_organizing(data, full)
        assert nearest is not None and brush.is_self_organizing(full[nearest]), name
        if brush.is_self_organizing(data) and not data.get("climbing"):
            assert nearest == name
    broadleaf = full[brush.nearest_self_organizing(full["broadleaf"], full)]
    assert broadleaf["architecture"]["branching_pattern"] != "rhythmic"
    assert brush.nearest_self_organizing(full["pine_like"], full) == "excurrent_conifer"
