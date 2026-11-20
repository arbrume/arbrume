"""Steps of the in-Blender test for the Arbrume Brush, and its before-and-after renders.

Imported by ``run_in_blender.py`` inside Blender; every step takes the extension module and
the ``check`` function of the runner. Strokes are given as world points, as the tool
would project them, through :func:`brush.add_stroke` or the stroke operator.
"""

from __future__ import annotations

import json
import time
from itertools import pairwise
from pathlib import Path

import bpy
import numpy as np
from bpy_extras import view3d_utils

SPECIES = "open_grown_broadleaf"
AGE = 20
LIMB_WAVE = np.stack(
    [np.zeros(40), np.zeros(40), np.sin(np.linspace(0.0, 2.0 * np.pi, 40)) * 1.2], axis=1
)
"""Vertical swing of the S-curve limb of the renders."""


def new_plant(preset: str = SPECIES, seed: int = 3, age: int = AGE, quality: float = 0.3):
    """Adds a plant at the world origin and returns it."""
    bpy.context.scene.cursor.location = (0.0, 0.0, 0.0)
    result = bpy.ops.arbrume.add_plant(
        preset=preset, seed=seed, use_species_age=False, age=age, quality=quality
    )
    assert result == {"FINISHED"}
    return bpy.context.view_layer.objects.active


def to_world(points) -> np.ndarray:
    """Plant-frame points (Y up) of a plant at the origin, in world space (Z up)."""
    points = np.asarray(points, dtype=np.float64).reshape(-1, 3)
    return np.stack([points[:, 0], -points[:, 2], points[:, 1]], axis=1)


def generated(ext, obj):
    """The tree the generator makes for a plant's settings and strokes."""
    settings = obj.arbrume
    overrides = ext.logic.values.load_overrides(settings.overrides)
    return ext.logic.species.generate(
        ext.plant.source_of(settings), overrides, **ext.plant.generation_args(settings)
    )


def live_nodes(tree) -> np.ndarray:
    """World positions of the live skeleton nodes of a tree grown at the origin."""
    skeleton = tree.skeleton
    live = ~np.asarray(skeleton.shed, dtype=bool)
    return to_world(np.asarray(skeleton.positions)[live])


def inside(points: np.ndarray, stroke: np.ndarray, radius: float) -> np.ndarray:
    """Which points lie within ``radius`` of a sample of the stroke polyline."""
    samples = stroke
    if len(stroke) > 1:
        samples = np.concatenate([np.linspace(a, b, 16) for a, b in pairwise(stroke)])
    distance = np.linalg.norm(points[:, None, :] - samples[None, :, :], axis=2)
    return distance.min(axis=1) <= radius


def s_curve(start, run: float, rise: float, swing: float, count: int = 40) -> np.ndarray:
    """World points of an S curve from ``start``: ``run`` along X, ``rise`` up, swinging
    ``swing`` along Y."""
    t = np.linspace(0.0, 1.0, count)
    wave = np.sin(t * 2.0 * np.pi) * swing
    return np.asarray(start) + np.stack([t * run, wave, t * rise], axis=1)


def settings(**values):
    """The brush settings of the scene with ``values`` set."""
    brush = bpy.context.scene.arbrume_brush
    for name, value in values.items():
        setattr(brush, name, value)
    return brush


def test_parametric_hint(ext, check) -> None:
    """A parametric plant is not steerable and switches to a self-organizing preset."""
    obj = new_plant("broadleaf", age=10)
    check(not ext.brush.is_steerable(obj), "a parametric plant is not steerable")
    nearest = ext.brush.nearest_preset(obj)
    check(nearest is not None, f"nearest self-organizing preset: {nearest}")
    result = bpy.ops.arbrume.switch_self_organizing()
    check(result == {"FINISHED"} and obj.arbrume.preset_name == nearest, "switch preset")
    check(ext.brush.is_steerable(obj), "the switched plant is steerable")
    bpy.data.objects.remove(obj)


def test_crown(ext, check) -> None:
    """A crown stroke painted to one side fills it."""
    obj = new_plant()
    before = live_nodes(generated(ext, obj))
    stroke = np.array([[5.0, 0.0, 8.0], [8.0, 0.0, 8.5]])
    brush = settings(mode="CROWN", radius=1.5, strength=2.0, reach=6.0, density=1.0)
    error = ext.brush.add_stroke(obj, "CROWN", stroke, brush)
    check(not error, f"crown stroke grows ({error})")
    after = live_nodes(generated(ext, obj))
    counts = (int(inside(before, stroke, 1.5).sum()), int(inside(after, stroke, 1.5).sum()))
    check(counts[1] > 3 * counts[0], f"nodes in the painted volume {counts[0]} -> {counts[1]}")
    widths = (before[:, 0].max(), after[:, 0].max())
    check(widths[1] > widths[0] + 1.5, f"crown reaches x {widths[0]:.2f} -> {widths[1]:.2f}")
    report = ext.state.status_of(obj.name).guides["strokes"][0]
    share = report["reached"] / max(report["markers"], 1)
    check(share > 2 / 3, f"painted markers reached: {share:.0%}")
    bpy.data.objects.remove(obj)


def test_limb(ext, check) -> None:
    """A limb drawn as an S curve from the trunk is followed by one branch."""
    obj = new_plant()
    stroke = s_curve((0.0, 0.0, 3.0), 5.0, 2.5, 1.0)
    brush = settings(mode="LIMB", radius=0.4, follow=1.0, density=1.0)
    check(not ext.brush.add_stroke(obj, "LIMB", stroke, brush), "limb stroke grows")
    report = ext.state.status_of(obj.name).guides["strokes"][0]
    check(report["claimed"], "a branch takes the limb")
    followed = report["followed"] / report["length"]
    check(followed > 0.9, f"branch grown along {followed:.0%} of the limb")
    distance = (report["mean_distance"], report["max_distance"])
    check(distance[0] < 0.15 and distance[1] < 0.6, f"axis to stroke {distance} m")
    nodes = live_nodes(generated(ext, obj))
    gaps = np.linalg.norm(stroke[:, None, :] - nodes[None, :, :], axis=2).min(axis=1)
    check(np.median(gaps) < 0.15, f"stroke covered by nodes: median gap {np.median(gaps):.3f} m")
    bpy.data.objects.remove(obj)


def test_erase(ext, check) -> None:
    """An erase stroke leaves its volume empty of growth. The volume is the union of balls
    along the stroke every half radius, whose waist is 0.968 radius wide, so the check
    stays within 0.95 radius of the line."""
    obj = new_plant()
    stroke = np.array([[3.0, 0.0, 5.0], [3.0, 0.0, 10.0]])
    before = int(inside(live_nodes(generated(ext, obj)), stroke, 1.5).sum())
    check(not ext.brush.add_stroke(obj, "ERASE", stroke, settings(mode="ERASE"), 1.5), "erase")
    after = int(inside(live_nodes(generated(ext, obj)), stroke, 0.95 * 1.5).sum())
    check(before > 50 and after == 0, f"nodes in the erased volume {before} -> {after}")
    bpy.data.objects.remove(obj)


def stroke_count(obj) -> int:
    """Number of strokes stored on a plant."""
    return len(json.loads(obj.arbrume.guides or '{"strokes": []}')["strokes"])


def stroke_step(**properties) -> bool:
    """Runs the stroke operator as the tool does on release, then pushes the undo step
    Blender pushes after a tool operator (an operator called from a script pushes none)."""
    finished = bpy.ops.arbrume.brush_stroke(**properties) == {"FINISHED"}
    bpy.ops.ed.undo_push(message="Arbrume Brush Stroke")
    return finished


def test_undo(ext, check) -> None:
    """Each stroke is one undo step that undo and redo restore with the mesh."""
    obj = new_plant(quality=0.2)
    name = obj.name
    bpy.ops.ed.undo_push(message="plant added")
    settings(radius=1.2, strength=2.0, reach=4.0)
    first = json.dumps(to_world([[3.0, 7.0, 0.0], [4.0, 7.5, 0.0]]).tolist())
    second = json.dumps(to_world([[-4.0, 6.0, 1.0]]).tolist())
    check(stroke_step(mode="CROWN", points=first), "stroke 1")
    one = (bpy.data.objects[name].arbrume.guides, len(bpy.data.objects[name].data.polygons))
    check(stroke_step(mode="ERASE", points=second), "stroke 2")
    two = (bpy.data.objects[name].arbrume.guides, len(bpy.data.objects[name].data.polygons))
    check(stroke_count(bpy.data.objects[name]) == 2 and one != two, "two strokes stored")
    bpy.ops.ed.undo()
    obj = bpy.data.objects[name]
    check((obj.arbrume.guides, len(obj.data.polygons)) == one, "undo removes the last stroke")
    bpy.ops.ed.undo()
    obj = bpy.data.objects[name]
    check(stroke_count(obj) == 0, "a second undo removes the first stroke")
    bpy.ops.ed.redo()
    bpy.ops.ed.redo()
    obj = bpy.data.objects[name]
    check((obj.arbrume.guides, len(obj.data.polygons)) == two, "redo restores both strokes")
    expected = generated(ext, obj).stats["triangles"]
    check(len(obj.data.polygons) == expected, "mesh after redo is the generated one")
    check(bpy.ops.arbrume.clear_strokes() == {"FINISHED"} and not obj.arbrume.guides, "clear")
    bpy.data.objects.remove(obj)


def test_freeze(ext, check) -> None:
    """Freezing keeps the plant grown so far; later strokes only add growth."""
    obj = new_plant(age=14)
    old = generated(ext, obj).skeleton
    check(bpy.ops.arbrume.freeze_growth() == {"FINISHED"}, "freeze")
    check(obj.arbrume.age == 14 + bpy.context.scene.arbrume_brush.grow_after_freeze, "age")
    ext.brush.add_stroke(obj, "CROWN", [[4.0, 1.0, 6.0]], settings(mode="CROWN", radius=1.0))
    stroke = json.loads(obj.arbrume.guides)["strokes"][0]
    check(stroke.get("cycle") == 14, "a stroke after a freeze acts from the frozen age")
    new = generated(ext, obj).skeleton
    born = np.asarray(new.births) < 14
    kept = np.asarray(new.positions)[born]
    original = np.asarray(old.positions)
    same = kept.shape == original.shape and np.array_equal(
        kept[np.lexsort(kept.T)], original[np.lexsort(original.T)]
    )
    check(same, f"frozen skeleton unchanged ({len(original)} nodes)")
    check(int((~born).sum()) > 0, f"{int((~born).sum())} nodes added after the freeze")
    check(bpy.ops.arbrume.unfreeze_growth() == {"FINISHED"}, "unfreeze")
    check(obj.arbrume.age == 14 and not obj.arbrume.guides, "unfreeze restores the age")
    bpy.data.objects.remove(obj)


def test_save_reload(ext, check, work: Path) -> None:
    """Strokes are saved with the file and the plant reopens editable."""
    obj = new_plant(quality=0.2)
    ext.brush.add_stroke(obj, "LIMB", s_curve((0, 0, 3), 4.0, 2.0, 0.8), settings(radius=0.4))
    name, guides, faces = obj.name, obj.arbrume.guides, len(obj.data.polygons)
    path = str(work / "brush.blend")
    bpy.ops.wm.save_as_mainfile(filepath=path)
    bpy.ops.wm.open_mainfile(filepath=path)
    obj = bpy.data.objects[name]
    check(obj.arbrume.guides == guides, "strokes reopen with the file")
    check(not ext.plant.regenerate(obj) and len(obj.data.polygons) == faces, "same plant")
    bpy.data.objects.remove(obj)


def time_strokes(ext) -> dict:
    """Generation and upload times of a stroke drawn in steps, at the preview quality and
    on release, for plants of three sizes."""
    timings = {}
    for preset, age in (("envelope_tree", 16), (SPECIES, 20), (SPECIES, 30)):
        obj = new_plant(preset, age=age, quality=1.0)
        brush = settings(mode="CROWN", radius=1.2, strength=2.0, reach=4.0, preview_quality=0.25)
        stroke = s_curve((2.0, 0.0, 5.0), 3.0, 2.0, 1.0, count=12)
        previews = []
        for end in range(2, len(stroke) + 1):
            guides = ext.brush.guides_with(obj, "CROWN", stroke[:end], brush)
            start = time.perf_counter()
            ext.plant.regenerate(obj, {"guides": guides, "quality": 0.25})
            previews.append((time.perf_counter() - start) * 1e3)
        start = time.perf_counter()
        ext.brush.add_stroke(obj, "CROWN", stroke, brush)
        final = (time.perf_counter() - start) * 1e3
        status = ext.state.status_of(obj.name)
        timings[f"{preset} age {age}"] = {
            "preview_ms_median": round(float(np.median(previews)), 1),
            "preview_ms_max": round(max(previews), 1),
            "release_ms": round(final, 1),
            "release_generate_ms": round(status.generate_ms, 1),
            "release_upload_ms": round(status.upload_ms, 1),
            "triangles": status.triangles,
        }
        bpy.data.objects.remove(obj)
    return timings


def run(ext, check, work: Path) -> None:
    """Every step, each in a scene of its own objects."""
    test_parametric_hint(ext, check)
    test_crown(ext, check)
    test_limb(ext, check)
    test_erase(ext, check)
    test_undo(ext, check)
    test_freeze(ext, check)
    test_save_reload(ext, check, work)
    print("brush timings (ms):", json.dumps(time_strokes(ext), indent=1), flush=True)


def stroke_object(name: str, points, kind: str, radius: float) -> bpy.types.Object:
    """A tube along world points, coloured by stroke kind, to show a stroke in a render."""
    colors = {"crown": (0.2, 0.9, 0.3, 1), "limb": (1.0, 0.45, 0.05, 1), "erase": (1, 0.1, 0.1, 1)}
    curve = bpy.data.curves.new(name, "CURVE")
    curve.dimensions = "3D"
    curve.bevel_depth = radius
    spline = curve.splines.new("POLY")
    points = np.asarray(points, dtype=np.float64)
    if len(points) == 1:
        points = np.concatenate([points, points + np.array([0.0, 0.0, 0.05])])
    spline.points.add(len(points) - 1)
    for point, co in zip(spline.points, points, strict=True):
        point.co = (*co, 1.0)
    material = bpy.data.materials.new(name)
    material.use_nodes = True
    node = material.node_tree.nodes["Principled BSDF"]
    node.inputs["Base Color"].default_value = colors[kind]
    node.inputs["Emission Color"].default_value = colors[kind]
    node.inputs["Emission Strength"].default_value = 1.5
    node.inputs["Alpha"].default_value = 1.0 if kind == "limb" else 0.2
    if hasattr(material, "surface_render_method"):
        material.surface_render_method = "BLENDED"
    curve.materials.append(material)
    obj = bpy.data.objects.new(name, curve)
    bpy.context.scene.collection.objects.link(obj)
    return obj


VIEW = (6.0, 26.0, -1.0)
"""Direction of the render camera, along which the erase stroke cuts."""
CASES = {
    "crown": {
        "strokes": [
            (
                "CROWN",
                [[3.0, 0.0, 5.5], [6.0, 0.0, 6.5], [8.0, 0.0, 8.5], [6.5, 0.0, 10.5]],
                2.0,
            ),
        ],
        "brush": {"strength": 2.0, "reach": 4.0, "density": 1.0},
        "freeze": (14, 8),
    },
    "limb": {
        "strokes": [
            ("LIMB", (s_curve((0.0, 0.0, 1.8), 7.0, 0.8, 0.0) + LIMB_WAVE).tolist(), 0.4),
        ],
        "brush": {"follow": 1.0, "density": 0.4, "strength": 1.5},
    },
    "erase": {
        "strokes": [("ERASE", [[2.5, 0.0, 5.0], [2.5, 0.0, 12.0]], 1.1)],
        "brush": {"erase_through": True},
    },
}
"""The strokes of the renders: a crown painted to one side of a plant frozen young, an S
curve limb low under the crown, and a gap erased through the crown along the view."""


def render_case(ext, name: str, case: dict, shoot) -> None:
    """Renders one plant before and after its strokes (the strokes shown as tubes: the
    volume of a crown or erase stroke, the line of a limb), then without the tubes. With
    ``freeze``, the plant shown before is frozen at the first age and grows the second
    number of cycles more after its strokes."""
    frozen, extra = case.get("freeze", (AGE, 0))
    obj = new_plant(seed=3, age=frozen, quality=1.0)
    shoot(f"brush_{name}_before.png")
    if extra:
        settings(grow_after_freeze=extra)
        bpy.ops.arbrume.freeze_growth()
    shown = []
    for index, (mode, points, radius) in enumerate(case["strokes"]):
        brush = settings(mode=mode, radius=radius, **case["brush"])
        start = time.perf_counter()
        error = ext.brush.add_stroke(obj, mode, points, brush, view=np.array(VIEW))
        elapsed = (time.perf_counter() - start) * 1e3
        print(f"render {name}: stroke {index} {elapsed:.0f} ms {error}", flush=True)
        tube = 0.05 if mode == "LIMB" else radius
        shown.append(stroke_object(f"{name} stroke {index}", points, mode.lower(), tube))
    print(f"render {name}: {ext.state.status_of(obj.name).guides}", flush=True)
    shoot(f"brush_{name}_after.png")
    for item in shown:
        item.hide_render = True
    shoot(f"brush_{name}_plain.png")
    for item in shown:
        bpy.data.objects.remove(item)
    bpy.data.objects.remove(obj)


def render_stills(ext, out: Path) -> None:
    """Before-and-after EEVEE stills of the three render cases, every other mesh hidden;
    uses the camera, light and ground of the runner's render scene."""
    scene = bpy.context.scene
    for obj in scene.objects:
        if obj.type in {"MESH", "CURVE"} and obj.name != "ground":
            obj.hide_render = True
    camera = scene.camera
    target = np.array([1.0, 0.0, 6.5])

    def shoot(filename: str) -> None:
        location = target + np.array([-6.0, -26.0, 1.0])
        camera.location = tuple(location)
        direction = type(camera.location)(tuple(target - location))
        camera.rotation_euler = direction.to_track_quat("-Z", "Y").to_euler()
        scene.render.filepath = str(out / filename)
        bpy.ops.render.render(write_still=True)

    for name, case in CASES.items():
        render_case(ext, name, case, shoot)


def screen_point(region, rv3d, world) -> tuple[int, int]:
    """Window coordinates of a world point seen in a 3D view region."""
    local = view3d_utils.location_3d_to_region_2d(region, rv3d, tuple(world))
    return int(region.x + local.x), int(region.y + local.y)


def ui_steps(ext, window, area, out: Path, check):
    """Draws a limb with simulated mouse events in a window, with the brush tool, and
    checks it is one undo step; yields timer intervals. Needs ``--enable-event-simulate``."""
    region = next(r for r in area.regions if r.type == "WINDOW")
    rv3d = area.spaces.active.region_3d
    with bpy.context.temp_override(window=window, area=area, region=region):
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete()
    obj = new_plant(age=16, quality=0.3)
    name = obj.name
    obj.select_set(False)
    keep = (ext.ui.VIEW3D_PT_arbrume, ext.brush.VIEW3D_PT_arbrume_brush)
    hidden = [c for c in ext._REGISTERED if issubclass(c, bpy.types.Panel) and c not in keep]
    for cls in hidden:
        bpy.utils.unregister_class(cls)
    rv3d.view_perspective = "PERSP"
    rv3d.view_rotation = (0.7071, 0.7071, 0.0, 0.0)
    rv3d.view_location = (1.5, 0.0, 5.0)
    rv3d.view_distance = 15.0
    settings(mode="LIMB", radius=0.5, follow=1.0, density=0.6, preview_quality=0.25)
    with bpy.context.temp_override(window=window, area=area, region=region):
        bpy.ops.wm.tool_set_by_id(name=ext.brush.TOOL_ID)
        bpy.ops.ed.undo_push(message="before the stroke")
    yield 1.0
    check(ext.brush.tool_active(bpy.context), "the brush tool is active")
    stroke = s_curve((0.0, 0.0, 3.0), 5.0, 3.0, 0.0, count=30)
    stroke[:, 2] += np.sin(np.linspace(0.0, 2.0 * np.pi, 30)) * 1.0
    pixels = [screen_point(region, rv3d, p) for p in stroke]
    window.event_simulate(type="MOUSEMOVE", value="NOTHING", x=pixels[0][0], y=pixels[0][1])
    window.event_simulate(type="LEFTMOUSE", value="PRESS", x=pixels[0][0], y=pixels[0][1])
    for start in (1, 8, 14):
        for x, y in pixels[start : start + 6]:
            window.event_simulate(type="MOUSEMOVE", value="NOTHING", x=x, y=y)
        yield 0.5
    with bpy.context.temp_override(window=window, area=area, region=region):
        bpy.ops.screen.screenshot(filepath=str(out / "brush_tool_drawing.png"))
    for x, y in pixels[20:]:
        window.event_simulate(type="MOUSEMOVE", value="NOTHING", x=x, y=y)
    window.event_simulate(type="LEFTMOUSE", value="RELEASE", x=pixels[-1][0], y=pixels[-1][1])
    yield 2.0
    obj = bpy.data.objects[name]
    check(stroke_count(obj) == 1, f"a simulated drag stores one stroke ({stroke_count(obj)})")
    timing = ext.brush.TIMINGS.get(name)
    if timing is not None:
        previews = [round(t) for t in timing.previews]
        print(f"drag: previews {previews} ms, release {timing.final:.0f} ms", flush=True)
        check(len(previews) > 0, "the plant grew while the button was held")
    report = ext.state.status_of(name).guides or {"strokes": [{}]}
    check(report["strokes"][0].get("claimed", False), "a branch takes the drawn limb")
    with bpy.context.temp_override(window=window, area=area, region=region):
        bpy.ops.screen.screenshot(filepath=str(out / "brush_tool_after.png"))
        bpy.ops.ed.undo()
    yield 1.0
    check(stroke_count(bpy.data.objects[name]) == 0, "undo removes the drawn stroke")
    with bpy.context.temp_override(window=window, area=area, region=region):
        bpy.ops.ed.redo()
    yield 1.0
    check(stroke_count(bpy.data.objects[name]) == 1, "redo brings the stroke back")
    for cls in hidden:
        bpy.utils.register_class(cls)
