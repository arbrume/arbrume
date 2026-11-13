"""End-to-end test of the extension inside Blender, from a built extension zip.

Run it with Python (not inside Blender)::

    uv run blender/tests/run_in_blender.py --blender "path/to/blender" \
        --zip blender/dist/arbrume-0.1.0-windows_x64.zip --out some/dir

It creates a temporary Blender user directory (``BLENDER_USER_RESOURCES``), installs the
zip there with ``blender --command extension install-file``, then runs itself inside
Blender in background mode. There it creates one plant per preset and checks its vertex
and triangle counts against ``tree.stats``, changes properties and parameters, edits a
curve through its widget node, scrubs the age, runs every operator, saves and reopens the
file, disables and enables the extension, times generation against mesh upload for a
large tree and renders two stills into ``--out``. The exit code is the number of failed
checks (0 on success).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
import traceback
from pathlib import Path

try:
    import addon_utils
    import bpy
    import numpy as np
    from mathutils import Vector
except ImportError:
    addon_utils = bpy = np = Vector = None

MODULE = "bl_ext.user_default.arbrume"
FAILURES: list[str] = []


def check(condition: bool, message: str) -> None:
    """Records a failed check and prints every check."""
    print(("ok   " if condition else "FAIL ") + message, flush=True)
    if not condition:
        FAILURES.append(message)


def launch(argv: list[str]) -> int:
    """Outside Blender: installs the zip into a temporary user directory and runs the test."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--blender", default=os.environ.get("BLENDER"), required=False)
    parser.add_argument("--zip", required=True)
    parser.add_argument("--out", default=tempfile.gettempdir())
    parser.add_argument("--ui", action="store_true", help="open a window and draw the panels")
    args = parser.parse_args(argv)
    if not args.blender:
        parser.error("give --blender or set BLENDER")
    with tempfile.TemporaryDirectory(prefix="arbrume-blender-") as user:
        env = dict(os.environ, BLENDER_USER_RESOURCES=user)
        install = [args.blender, "--command", "extension", "install-file", "-r", "user_default"]
        subprocess.run([*install, "-e", args.zip], env=env, check=True)
        mode = [] if args.ui else ["--background"]
        test = [args.blender, *mode, "--factory-startup", "--python-exit-code", "1"]
        test += ["--python", __file__, "--", "--out", args.out, "--work", user]
        if not args.ui:
            return subprocess.run(test, env=env, check=False).returncode
        result = subprocess.run(test, env=env, check=False, capture_output=True, text=True)
        output = result.stdout + result.stderr
        print(output)
        tracebacks = output.count("Traceback")
        print(f"{tracebacks} tracebacks while drawing")
        return result.returncode + tracebacks + int("UI checks passed" not in output)


# Everything below runs inside Blender.


def extension():
    """The extension package module, enabled."""
    if MODULE not in bpy.context.preferences.addons:
        addon_utils.enable(MODULE, default_set=True, handle_error=None)
    return sys.modules[MODULE]


def expected_stats(ext, obj) -> dict:
    """Stats of the tree the generator makes for a plant's settings."""
    settings = obj.arbrume
    source = ext.plant.source_of(settings)
    overrides = ext.logic.values.load_overrides(settings.overrides)
    tree = ext.logic.species.generate(source, overrides, **ext.plant.generation_args(settings))
    return tree.stats


def check_counts(ext, obj, label: str) -> None:
    """The mesh of a plant has the vertex and triangle counts of its tree."""
    stats = expected_stats(ext, obj)
    mesh = obj.data
    check(
        len(mesh.vertices) == stats["vertices"] and len(mesh.polygons) == stats["triangles"],
        f"{label}: {len(mesh.vertices)} vertices / {len(mesh.polygons)} triangles "
        f"(expected {stats['vertices']} / {stats['triangles']})",
    )


def check_normals(ext, obj) -> None:
    """Shading normals of the mesh are the normals computed by the generator."""
    settings = obj.arbrume
    tree = ext.logic.species.generate(
        ext.plant.source_of(settings), None, **ext.plant.generation_args(settings)
    )
    arrays = ext.logic.meshdata.build_arrays(tree.parts)
    mesh = obj.data
    corner = np.empty(3 * len(mesh.loops), dtype=np.float32)
    mesh.corner_normals.foreach_get("vector", corner)
    expected = arrays.normals[arrays.corner_verts].ravel()
    error = float(np.abs(corner - expected).max())
    check(mesh.has_custom_normals and error < 1e-2, f"custom normals match (max error {error:.4f})")


def add_plant(ext, preset: str, **kwargs):
    """Runs the Add operator and returns the new plant."""
    result = bpy.ops.arbrume.add_plant(preset=preset, **kwargs)
    check(result == {"FINISHED"}, f"add_plant {preset}")
    return bpy.context.view_layer.objects.active


def test_presets(ext) -> None:
    """One plant per preset, with counts, materials and attributes."""
    for index, preset in enumerate(ext.state.STATE.presets):
        obj = add_plant(ext, preset, seed=index, quality=0.25, use_species_age=False, age=8)
        obj.location.x = index * 3.0
        check_counts(ext, obj, f"preset {preset}")
        mesh = obj.data
        check(len(mesh.materials) == 2, f"{preset}: two material slots")
        check(mesh.color_attributes.get("Col") is not None, f"{preset}: colour attribute")
        check(mesh.uv_layers.active is not None, f"{preset}: active UV map")
        if index == 0:
            check_normals(ext, obj)


def test_live_properties(ext, obj) -> None:
    """Changing settings and species parameters regenerates the mesh."""
    before = len(obj.data.polygons)
    obj.arbrume.seed = 12345
    check_counts(ext, obj, "after seed change")
    obj.arbrume.params.foliage__length = 0.3
    check("foliage.length" in json.loads(obj.arbrume.overrides), "parameter writes override")
    check_counts(ext, obj, "after foliage.length change")
    obj.arbrume.params.bark__color = (0.5, 0.4, 0.3)
    check(obj.data.materials[0].diffuse_color[0] > 0.45, "bark colour reaches the material")
    obj.arbrume.params.foliage__shape = "lanceolate"
    check(json.loads(obj.arbrume.overrides).get("foliage.shape") == "lanceolate", "enum override")
    obj.arbrume.curve_order = 1
    obj.arbrume.params.growth__branching_angle = 30.0
    value = json.loads(obj.arbrume.overrides)["growth.branching_angle"]
    check(isinstance(value, dict) and value["by_order"][1] == 30.0, f"order-1 curve set: {value}")
    check_counts(ext, obj, "after per-order constant")
    obj.arbrume.quality = 0.5
    check(len(obj.data.polygons) != before, "quality changes the mesh")


def test_curve_widget(ext, obj) -> None:
    """The curve node of a widget syncs both ways with the overrides."""
    curves = ext.curves_ui
    obj.arbrume.curve_order = 1
    bpy.ops.arbrume.curve_mode(path="growth.axis_length", to_curve=True)
    node = curves.request(obj, "growth.axis_length", 1)
    curves.sync_bindings()
    node = curves.request(obj, "growth.axis_length", 1)
    check(node is not None, "curve node created by the timer")
    if node is None:
        return
    points = node.mapping.curves[0].points
    points.new(0.5, 0.9)
    points[0].location = (0.0, 0.1)
    for point in points:
        point.handle_type = "VECTOR"
    node.mapping.update()
    curves.sync_bindings()
    value = json.loads(obj.arbrume.overrides)["growth.axis_length"]
    entry = value["by_order"][1] if isinstance(value, dict) else value
    check(isinstance(entry, list) and len(entry) >= 3, f"widget edit written: {entry}")
    check_counts(ext, obj, "after curve edit")
    bpy.ops.arbrume.reset_param(path="growth.axis_length")
    curves.request(obj, "growth.axis_length", 1)
    curves.sync_bindings()
    check("growth.axis_length" not in json.loads(obj.arbrume.overrides), "curve reset")


def test_age_scrub(ext, obj) -> None:
    """Every age gives the mesh of that age."""
    for age in range(0, 41, 5):
        obj.arbrume.age = age
        check_counts(ext, obj, f"age {age}")


def test_operators(ext, obj, work: Path) -> None:
    """Every operator on a plant."""
    view_layer = bpy.context.view_layer
    view_layer.objects.active = obj
    obj.select_set(True)
    seed = obj.arbrume.seed
    check(bpy.ops.arbrume.randomize_seed() == {"FINISHED"}, "randomize_seed")
    check(obj.arbrume.seed != seed, "seed changed")
    check(bpy.ops.arbrume.regenerate() == {"FINISHED"}, "regenerate")
    check(bpy.ops.arbrume.duplicate_new_seed() == {"FINISHED"}, "duplicate_new_seed")
    copy = view_layer.objects.active
    check(copy != obj and copy.data != obj.data, "duplicate has its own mesh")
    check_counts(ext, copy, "duplicate")
    view_layer.objects.active = obj
    check(bpy.ops.arbrume.scatter(count=6, variations=2) == {"FINISHED"}, "scatter")
    scattered = bpy.data.collections[f"{obj.name} scatter"].objects
    check(len(scattered) == 6 and len({o.data for o in scattered}) == 2, "scatter shares meshes")
    check_counts(ext, scattered[0], "scattered plant")
    check(bpy.ops.arbrume.make_lods(levels=3) == {"FINISHED"}, "make_lods")
    lods = sorted(bpy.data.collections[f"{obj.name} LODs"].objects, key=lambda o: o.name)
    tris = [len(o.data.polygons) for o in lods]
    check(len(lods) == 3 and tris[0] > tris[1] > tris[2], f"LOD triangles decrease: {tris}")
    path = str(work / "exported.toml")
    check(bpy.ops.arbrume.export_species(filepath=path) == {"FINISHED"}, "export_species")
    check(bpy.ops.arbrume.import_species(filepath=path, new_plant=True) == {"FINISHED"}, "import")
    imported = view_layer.objects.active
    for name in ("seed", "age", "scale", "quality"):
        setattr(imported.arbrume, name, getattr(obj.arbrume, name))
    check(len(imported.data.polygons) == len(obj.data.polygons), "imported species = original")
    check(bpy.ops.arbrume.reset_param(path="") == {"FINISHED"}, "reset all")


def test_errors(ext, obj) -> None:
    """Invalid input reports the generator message instead of raising."""
    obj.arbrume.overrides = '{"growth.no_such_field": 1}'
    error = ext.state.status_of(obj.name).error
    check("no_such_field" in error, f"error shown: {error.splitlines()[0] if error else ''}")
    try:
        bpy.ops.arbrume.regenerate()
        check(False, "regenerate reports the error")
    except RuntimeError as failure:
        check("no_such_field" in str(failure), "regenerate reports the error")
    obj.arbrume.overrides = "{}"
    check(not ext.state.status_of(obj.name).error, "error cleared")


def test_save_reload(ext, work: Path) -> None:
    """A saved file reopens with editable plants."""
    path = str(work / "plants.blend")
    plants = {o.name: len(o.data.polygons) for o in bpy.data.objects if ext.plant.is_plant(o)}
    bpy.ops.wm.save_as_mainfile(filepath=path)
    bpy.ops.wm.open_mainfile(filepath=path)
    reopened = [o for o in bpy.data.objects if ext.plant.is_plant(o)]
    check(len(reopened) == len(plants), f"{len(reopened)} plants reopened")
    obj = reopened[0]
    check(obj.arbrume.params is not None, "parameters available after reload")
    check(not ext.plant.regenerate(obj), "regenerate after reload")
    check(len(obj.data.polygons) == plants[obj.name], "same mesh after reload")
    obj.arbrume.seed += 1
    check_counts(ext, obj, "edit after reload")


def test_reregister(ext) -> None:
    """Disabling and enabling the extension keeps plants editable."""
    addon_utils.disable(MODULE, default_set=True)
    check(not hasattr(bpy.types.Object, "arbrume"), "unregister removes Object.arbrume")
    addon_utils.enable(MODULE, default_set=True)
    ext = sys.modules[MODULE]
    obj = next(o for o in bpy.data.objects if ext.plant.is_plant(o))
    check(not ext.plant.regenerate(obj), "regenerate after re-register")
    return ext


def time_upload(ext) -> dict:
    """Milliseconds of generation and mesh upload of a large tree, best of three."""
    obj = add_plant(ext, "oak_like", seed=1, quality=1.0, use_species_age=False, age=60)
    timings = {}
    for normals in (True, False):
        bpy.context.preferences.addons[MODULE].preferences.custom_normals = normals
        best_gen = best_up = float("inf")
        for _ in range(3):
            ext.plant.regenerate(obj)
            status = ext.state.status_of(obj.name)
            best_gen = min(best_gen, status.generate_ms)
            best_up = min(best_up, status.upload_ms)
        timings["custom_normals" if normals else "no_custom_normals"] = (best_gen, best_up)
    status = ext.state.status_of(obj.name)
    timings["size"] = (status.vertices, status.triangles)
    bpy.context.preferences.addons[MODULE].preferences.custom_normals = True
    bpy.data.objects.remove(obj)
    return timings


def render_stills(ext, out: Path) -> None:
    """Two EEVEE stills: a broadleaf tree and a group of three species."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    ext = extension()
    scene = bpy.context.scene
    engines = bpy.types.RenderSettings.bl_rna.properties["engine"].enum_items.keys()
    scene.render.engine = next(e for e in ("BLENDER_EEVEE_NEXT", "BLENDER_EEVEE") if e in engines)
    scene.render.resolution_x, scene.render.resolution_y = 720, 720
    world = bpy.data.worlds.new("sky")
    if not world.use_nodes:
        world.use_nodes = True
    world.node_tree.nodes["Background"].inputs["Color"].default_value = (0.5, 0.65, 0.9, 1)
    world.node_tree.nodes["Background"].inputs["Strength"].default_value = 0.8
    scene.world = world
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 4.0
    sun.rotation_euler = (0.9, 0.0, 0.7)
    scene.collection.objects.link(sun)
    ground_mesh = bpy.data.meshes.new("ground")
    ground_mesh.from_pydata(
        [(-80, -80, 0), (80, -80, 0), (80, 80, 0), (-80, 80, 0)], [], [(0, 1, 2, 3)]
    )
    ground_mat = bpy.data.materials.new("ground")
    ground_mat.diffuse_color = (0.25, 0.22, 0.15, 1)
    ground_mat.node_tree.nodes["Principled BSDF"].inputs[0].default_value = (0.25, 0.22, 0.15, 1)
    ground_mesh.materials.append(ground_mat)
    scene.collection.objects.link(bpy.data.objects.new("ground", ground_mesh))
    camera = bpy.data.objects.new("camera", bpy.data.cameras.new("camera"))
    scene.collection.objects.link(camera)
    scene.camera = camera

    def shoot(name: str, target: Vector, distance: float, height: float) -> None:
        camera.location = target + Vector((0.0, -distance, height))
        camera.rotation_euler = (target - camera.location).to_track_quat("-Z", "Y").to_euler()
        scene.render.filepath = str(out / name)
        bpy.ops.render.render(write_still=True)

    tree = add_plant(ext, "broadleaf", seed=4, use_species_age=False, age=30, quality=1.0)
    height = tree.dimensions.z
    shoot("broadleaf.png", Vector((0, 0, height * 0.5)), height * 1.8, height * 0.15)
    tree.hide_render = True
    tallest = 0.0
    for i, preset in enumerate(("birch_like", "pine_like", "maple_like")):
        plant = add_plant(ext, preset, seed=7 + i, quality=0.6, use_species_age=False, age=30)
        plant.location.x = (i - 1) * 12.0
        tallest = max(tallest, plant.dimensions.z)
    shoot("group.png", Vector((0, 0, tallest * 0.5)), tallest * 3.0, 1.5)


def sidebar():
    """Window, 3D view area and sidebar region of the main window, sidebar opened.

    A script cannot choose the sidebar tab, so the window test moves the panels to the
    "Item" tab, which a new file shows.
    """
    window = bpy.context.window_manager.windows[0]
    area = max(
        (a for a in window.screen.areas if a.type == "VIEW_3D"), key=lambda a: a.width * a.height
    )
    area.spaces.active.show_region_ui = True
    region = next(r for r in area.regions if r.type == "UI")
    return window, area, region


def ui_steps(out: Path):
    """Steps of the window test, one per timer tick: add a plant, draw, screenshot, quit."""
    ext = extension()
    ext.ui.OPEN_ALL_GROUPS = True
    panels = [c for c in ext._REGISTERED if issubclass(c, bpy.types.Panel)]
    for cls in reversed(panels):
        bpy.utils.unregister_class(cls)
    for cls in panels:
        cls.bl_category = "Item"
        cls.bl_options = set()
        bpy.utils.register_class(cls)
    window, area, region = sidebar()
    with bpy.context.temp_override(window=window, area=area, region=region):
        bpy.ops.object.select_all(action="SELECT")
        bpy.ops.object.delete()
        bpy.ops.arbrume.add_plant(preset="broadleaf", quality=0.3)
    obj = bpy.context.view_layer.objects.active
    obj.arbrume.curve_order = 1
    obj.arbrume.params.growth__branching_angle = 40.0
    ext.props.write_curve_entry(
        obj.arbrume,
        ext.state.STATE.specs["growth.axis_length"],
        [[0.0, 1.0], [0.6, 4.0], [1.0, 2.0]],
    )
    yield 1.0
    region.tag_redraw()
    yield 1.0
    node = ext.curves_ui.find_node(ext.curves_ui.binding_name(obj.name, "growth.axis_length", 1))
    check(node is not None, "curve widget node created after drawing")
    check(len(ext.state.STATE.bindings) > 0, "panels requested curve widgets")
    for page in range(3):
        with bpy.context.temp_override(window=window, area=area, region=region):
            bpy.ops.screen.screenshot(filepath=str(out / f"sidebar_{page}.png"))
            bpy.ops.view2d.scroll_down(deltax=0, deltay=0, page=True)
        region.tag_redraw()
        yield 1.0
    if not FAILURES:
        print("UI checks passed", flush=True)
    bpy.ops.wm.quit_blender()
    yield None


def run_inside(argv: list[str]) -> int | None:
    """Inside Blender: every check, then the timings and the renders."""
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", required=True)
    parser.add_argument("--work", required=True)
    args = parser.parse_args(argv)
    out, work = Path(args.out), Path(args.work)
    out.mkdir(parents=True, exist_ok=True)
    if not bpy.app.background:
        steps = ui_steps(out)

        def tick():
            try:
                return next(steps)
            except Exception:
                traceback.print_exc()
                bpy.ops.wm.quit_blender()
                return None

        bpy.app.timers.register(tick, first_interval=1.0)
        return None
    bpy.ops.wm.read_factory_settings(use_empty=True)
    ext = extension()
    print("Blender", bpy.app.version_string, "arbrume", ext.arbrume.__version__, flush=True)
    test_presets(ext)
    obj = add_plant(ext, "broadleaf", seed=1, quality=0.3, use_species_age=False, age=12)
    for step in (test_live_properties, test_curve_widget, test_age_scrub, test_errors):
        step(ext, obj)
    test_operators(ext, obj, work)
    test_save_reload(ext, work)
    ext = test_reregister(ext)
    start = time.perf_counter()
    timings = time_upload(ext)
    print("timings (generate ms, upload ms):", timings, flush=True)
    print(f"timing pass took {time.perf_counter() - start:.1f} s", flush=True)
    render_stills(ext, out)
    print(f"{len(FAILURES)} failed checks", flush=True)
    for failure in FAILURES:
        print("  FAILED:", failure)
    return len(FAILURES)


if __name__ == "__main__":
    if bpy is None:
        sys.exit(launch(sys.argv[1:]))
    code = run_inside(sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else [])
    if code is not None:
        sys.exit(code)
