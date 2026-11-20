"""Generating a plant into the mesh of its Blender object.

A plant is one object with one mesh: the bark and leaf parts of the tree are two material
slots of that mesh. One mesh means one clear and one upload per regeneration, one
datablock to share between scattered copies and one object to move, duplicate or link.
The mesh is rebuilt in place (``clear_geometry``) with ``foreach_set`` from the arrays of
:mod:`.logic.meshdata`, so no Python loop touches a vertex.
"""

from __future__ import annotations

import hashlib
import os
import tempfile
import time
from dataclasses import dataclass

import bpy

from . import surroundings
from .logic import meshdata
from .logic.species import Source, generate
from .logic.values import load_overrides
from .state import STATE, status_of

COLOR_ATTRIBUTE = "Col"
UV_LAYER = "UVMap"
MANAGED_KEY = "arbrume_material"

# Test-only switches that force the paths taken where Blender lacks the faster APIs
# (Blender 4.2 to 4.x); the in-Blender test sets them, users never do.
FORCE_LOOP_VERTEX_INDEX = False
FORCE_SPLIT_NORMALS = False


@dataclass
class Preferences:
    """Settings of the extension, with defaults when its preferences are not available."""

    default_quality: float = 1.0
    threads: int = 0
    custom_normals: bool = True


def preferences() -> Preferences:
    """Current preferences of the extension."""
    addon = bpy.context.preferences.addons.get(__package__)
    prefs = getattr(addon, "preferences", None)
    if prefs is None:
        return Preferences()
    return Preferences(prefs.default_quality, prefs.threads, prefs.custom_normals)


def is_plant(obj: bpy.types.Object | None) -> bool:
    """Whether ``obj`` is a mesh object made by Arbrume."""
    settings = getattr(obj, "arbrume", None)
    return obj is not None and obj.type == "MESH" and settings is not None and settings.is_plant


def is_editable(obj: bpy.types.Object) -> bool:
    """Whether the mesh of ``obj`` can be rebuilt: local, and not in edit mode."""
    return obj.library is None and obj.data.library is None and obj.mode != "EDIT"


def source_of(settings: bpy.types.PropertyGroup) -> Source:
    """Species source of a plant."""
    if settings.custom_toml:
        return Source(toml=settings.custom_toml)
    return Source(preset=settings.preset_name)


def species_label(settings: bpy.types.PropertyGroup) -> str:
    """Name of a plant's species for display."""
    if settings.custom_toml:
        return settings.custom_name or "Custom species"
    return settings.preset_name


def generation_args(settings: bpy.types.PropertyGroup) -> dict:
    """Keyword arguments of ``arbrume.generate`` for a plant, except the species."""
    return {
        "seed": int(settings.seed),
        "age": int(settings.age),
        "scale": float(settings.scale),
        "quality": float(settings.quality),
        "textures": bool(getattr(settings, "textures", False)),
        "junctions": str(getattr(settings, "junctions", "tier-a")),
    }


def surroundings_of(obj: bpy.types.Object) -> tuple[dict | None, surroundings.Report]:
    """The ``surroundings`` argument of a plant (its envelope and obstacle objects, for a
    self-organizing species) and what building it took."""
    settings = obj.arbrume
    species = STATE.cache.effective_text(source_of(settings), settings.overrides)
    return surroundings.build(obj, species)


def _record(status, report: surroundings.Report) -> None:
    """Copies what building the surroundings took into the status of a plant."""
    status.surroundings_ms = report.milliseconds
    status.voxelized, status.cached = report.voxelized, report.cached
    status.filled, status.cell = report.filled, report.cell
    status.warning = "; ".join(report.errors)


def own_mesh(obj: bpy.types.Object) -> bpy.types.Mesh:
    """The mesh of ``obj``, replaced first by a new mesh if other objects share it."""
    mesh = obj.data
    if mesh.users > 1:
        fresh = bpy.data.meshes.new(mesh.name)
        for material in mesh.materials:
            fresh.materials.append(material)
        obj.data = fresh
        mesh = fresh
    return mesh


def _set_attribute(mesh, name: str, kind: str, domain: str, field: str, values) -> None:
    """Writes a whole attribute, creating it first when the mesh lacks it."""
    attribute = mesh.attributes.get(name) or mesh.attributes.new(name, kind, domain)
    attribute.data.foreach_set(field, values)


def _set_custom_normals(mesh: bpy.types.Mesh, normals) -> None:
    """Per-vertex custom normals: a ``custom_normal`` point attribute where Blender reads
    it as free normals (about 1 ms for 300 000 vertices), else the much slower
    ``normals_split_custom_set_from_vertices``."""
    _set_attribute(mesh, "custom_normal", "FLOAT_VECTOR", "POINT", "vector", normals.ravel())
    if FORCE_SPLIT_NORMALS or not mesh.has_custom_normals:
        mesh.attributes.remove(mesh.attributes["custom_normal"])
        mesh.normals_split_custom_set_from_vertices(normals)


def upload(mesh: bpy.types.Mesh, arrays: meshdata.MeshArrays, custom_normals: bool) -> None:
    """Replaces the geometry of ``mesh`` by ``arrays``.

    Element counts are set with ``add``; data goes through the generic attribute API
    (``position``, ``.corner_vert``, ``material_index``, the UV map), which copies a
    buffer in one call where the older ``vertices``/``loops``/``polygons`` properties go
    element by element.
    """
    mesh.clear_geometry()
    if arrays.face_count == 0:
        mesh.update()
        return
    mesh.vertices.add(arrays.vertex_count)
    mesh.loops.add(len(arrays.corner_verts))
    mesh.polygons.add(arrays.face_count)
    _set_attribute(mesh, "position", "FLOAT_VECTOR", "POINT", "vector", arrays.positions)
    if not FORCE_LOOP_VERTEX_INDEX and mesh.attributes.get(".corner_vert") is not None:
        mesh.attributes[".corner_vert"].data.foreach_set("value", arrays.corner_verts)
    else:
        mesh.loops.foreach_set("vertex_index", arrays.corner_verts)
    mesh.polygons.foreach_set("loop_start", arrays.face_starts)
    _set_attribute(mesh, "material_index", "INT", "FACE", "value", arrays.material_indices)
    mesh.update(calc_edges=True)
    mesh.shade_smooth()
    if arrays.corner_uvs is not None:
        _set_attribute(mesh, UV_LAYER, "FLOAT2", "CORNER", "vector", arrays.corner_uvs)
    if arrays.colors is not None:
        attribute = mesh.color_attributes.new(COLOR_ATTRIBUTE, "FLOAT_COLOR", "POINT")
        attribute.data.foreach_set("color", arrays.colors)
        mesh.color_attributes.active_color = attribute
        mesh.color_attributes.render_color_index = mesh.color_attributes.active_color_index
    if custom_normals and arrays.normals is not None:
        _set_custom_normals(mesh, arrays.normals)


def _socket(sockets, name: str, kind: str | None = None):
    for socket in sockets:
        if socket.name == name and (kind is None or socket.type == kind):
            return socket
    return None


def _image(name: str, png: bytes | None, color: bool) -> bpy.types.Image | None:
    """A packed image of the PNG bytes ``png``, reused when one of that name exists.

    Images are named after a hash of their bytes, so plants of a species share them;
    ``color`` images are sRGB, the others (normals, roughness) non-colour data.
    """
    if not png:
        return None
    key = f"Arbrume {name} {hashlib.sha1(png).hexdigest()[:10]}"
    image = bpy.data.images.get(key)
    if image is not None:
        return image
    with tempfile.TemporaryDirectory() as folder:
        path = os.path.join(folder, "texture.png")
        with open(path, "wb") as handle:
            handle.write(png)
        image = bpy.data.images.load(path, check_existing=False)
        image.pack()
    image.name = key
    image.colorspace_settings.name = "sRGB" if color else "Non-Color"
    return image


def _image_node(tree, image: bpy.types.Image, location: tuple[int, int]):
    node = tree.nodes.new("ShaderNodeTexImage")
    node.image = image
    node.location = location
    return node


def _textured(tree, data: dict, bsdf, color_input) -> None:
    """Wires the textures of ``data`` into ``bsdf``.

    The base colour image times the colour of ``color_input`` feeds that input, its alpha
    is cut at the alpha cutoff, the normal image goes through a tangent-space normal map
    and the roughness image (green channel) multiplies the roughness.
    """
    base = _image("base", data.get("base_color_texture"), True)
    if base is not None:
        node = _image_node(tree, base, (-700, 200))
        mix = tree.nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.blend_type = "MULTIPLY"
        mix.location = (-400, 200)
        mix.inputs[0].default_value = 1.0
        _socket(mix.inputs, "A", "RGBA").default_value = tuple(color_input.default_value)
        tree.links.new(node.outputs["Color"], _socket(mix.inputs, "B", "RGBA"))
        tree.links.new(_socket(mix.outputs, "Result", "RGBA"), color_input)
        cutoff = data.get("alpha_cutoff")
        if cutoff is not None:
            clip = tree.nodes.new("ShaderNodeMath")
            clip.operation = "GREATER_THAN"
            clip.inputs[1].default_value = float(cutoff)
            clip.location = (-400, -50)
            tree.links.new(node.outputs["Alpha"], clip.inputs[0])
            tree.links.new(clip.outputs[0], bsdf.inputs["Alpha"])
    normal = _image("normal", data.get("normal_texture"), False)
    if normal is not None:
        node = _image_node(tree, normal, (-700, -300))
        normal_map = tree.nodes.new("ShaderNodeNormalMap")
        normal_map.uv_map = UV_LAYER
        normal_map.location = (-400, -300)
        tree.links.new(node.outputs["Color"], normal_map.inputs["Color"])
        tree.links.new(normal_map.outputs["Normal"], bsdf.inputs["Normal"])
    rough = _image("roughness", data.get("roughness_texture"), False)
    if rough is not None:
        node = _image_node(tree, rough, (-700, -600))
        scale = tree.nodes.new("ShaderNodeMath")
        scale.operation = "MULTIPLY"
        scale.inputs[1].default_value = float(data.get("roughness", 1.0))
        scale.location = (-400, -600)
        tree.links.new(node.outputs["Color"], scale.inputs[0])
        tree.links.new(scale.outputs[0], bsdf.inputs["Roughness"])


def _build_material_nodes(material: bpy.types.Material, data: dict) -> None:
    """Principled BSDF whose base colour is the material colour times the vertex colour,
    times the base colour texture when the part has textures (see :func:`_textured`)."""
    color = list(data.get("base_color") or [0.8, 0.8, 0.8, 1.0])[:4]
    roughness = float(data.get("roughness", 0.5))
    tree = material.node_tree
    tree.nodes.clear()
    output = tree.nodes.new("ShaderNodeOutputMaterial")
    output.location = (400, 0)
    bsdf = tree.nodes.new("ShaderNodeBsdfPrincipled")
    bsdf.location = (100, 0)
    attribute = tree.nodes.new("ShaderNodeVertexColor")
    attribute.layer_name = COLOR_ATTRIBUTE
    attribute.location = (-400, -100)
    mix = tree.nodes.new("ShaderNodeMix")
    mix.data_type = "RGBA"
    mix.blend_type = "MULTIPLY"
    mix.location = (-150, 0)
    mix.inputs[0].default_value = 1.0
    _socket(mix.inputs, "A", "RGBA").default_value = color
    tree.links.new(attribute.outputs["Color"], _socket(mix.inputs, "B", "RGBA"))
    tree.links.new(_socket(mix.outputs, "Result", "RGBA"), bsdf.inputs["Base Color"])
    bsdf.inputs["Roughness"].default_value = roughness
    tree.links.new(bsdf.outputs[0], output.inputs["Surface"])
    _textured(tree, data, bsdf, _socket(mix.inputs, "A", "RGBA"))
    material.diffuse_color = color
    material.roughness = roughness
    material.use_backface_culling = not data.get("double_sided", False)


def material_for(species_name: str, kind: str, data: dict) -> bpy.types.Material:
    """The material of one part, created once per species and material values."""
    key = meshdata.material_key(species_name, kind, data)
    material = bpy.data.materials.get(key)
    if material is not None and material.library is None and material.get(MANAGED_KEY):
        return material
    material = bpy.data.materials.new(key)
    if not material.use_nodes:
        material.use_nodes = True
    material[MANAGED_KEY] = kind
    _build_material_nodes(material, data)
    return material


def assign_materials(mesh: bpy.types.Mesh, species_name: str, arrays: meshdata.MeshArrays):
    """Puts one material per part kind in the material slots of ``mesh``."""
    for index, (kind, data) in enumerate(zip(arrays.kinds, arrays.materials, strict=True)):
        material = material_for(species_name, kind, data)
        if index < len(mesh.materials):
            if mesh.materials[index] != material:
                mesh.materials[index] = material
        else:
            mesh.materials.append(material)


def fill_mesh(mesh: bpy.types.Mesh, tree) -> meshdata.MeshArrays:
    """Replaces the geometry and materials of ``mesh`` by those of a generated tree."""
    arrays = meshdata.build_arrays(tree.parts)
    upload(mesh, arrays, preferences().custom_normals)
    assign_materials(mesh, tree.species_name, arrays)
    return arrays


def mesh_from_tree(name: str, tree) -> bpy.types.Mesh:
    """A new mesh holding a generated tree."""
    mesh = bpy.data.meshes.new(name)
    fill_mesh(mesh, tree)
    return mesh


def apply_tree(obj: bpy.types.Object, tree) -> tuple[float, int, int]:
    """Uploads a generated tree into the mesh of ``obj``; returns milliseconds and sizes."""
    start = time.perf_counter()
    arrays = fill_mesh(own_mesh(obj), tree)
    elapsed = (time.perf_counter() - start) * 1e3
    return elapsed, arrays.vertex_count, arrays.face_count


def regenerate(obj: bpy.types.Object) -> str:
    """Generates the plant of ``obj`` from its properties; returns an error message or ""."""
    key = obj.as_pointer()
    if key in STATE.busy:
        return ""
    status = status_of(obj.name)
    if not is_editable(obj):
        status.error = "Plant cannot be rebuilt: it is linked or in edit mode"
        return status.error
    settings = obj.arbrume
    STATE.busy.add(key)
    try:
        around, report = surroundings_of(obj)
        _record(status, report)
        start = time.perf_counter()
        tree = generate(
            source_of(settings),
            load_overrides(settings.overrides),
            surroundings=around,
            **generation_args(settings),
        )
        status.generate_ms = (time.perf_counter() - start) * 1e3
        status.upload_ms, status.vertices, status.triangles = apply_tree(obj, tree)
        status.error = ""
    except (ValueError, TypeError, OSError, RuntimeError) as error:
        status.error = str(error).strip()
    finally:
        STATE.busy.discard(key)
    return status.error


def regenerate_if_live(obj: bpy.types.Object) -> None:
    """Regenerates ``obj`` after a property change, unless updates are suspended or off."""
    if STATE.suspended or not is_plant(obj) or not obj.arbrume.live_update:
        return
    regenerate(obj)
