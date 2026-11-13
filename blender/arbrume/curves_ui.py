"""Curve widgets for curve parameters, kept in sync with the overrides.

Blender only draws a curve widget (``layout.template_curve_mapping``) for a curve that
belongs to a datablock, so each edited curve gets a Float Curve node in a private node
group (``.Arbrume Curves``, hidden from lists and not kept when unused). Panels cannot
change data while drawing: they only record which curves are shown, and a timer creates
the nodes and syncs both ways. A node edited by the user writes its points to the
override of its plant; an override changed elsewhere (undo, reset, a species file) is
written back to the node.
"""

from __future__ import annotations

import hashlib
import time

import bpy

from . import plant, props
from .logic import curvemap
from .logic.values import entry_at, entry_points, points_entry
from .state import STATE, CurveBinding, status_of

NODE_TREE = ".Arbrume Curves"
SYNC_INTERVAL = 0.2
STALE_SECONDS = 10.0


def binding_name(object_name: str, path: str, order: int) -> str:
    """Name of the node holding one curve of one plant."""
    key = f"{object_name}\x00{path}\x00{order}"
    return "curve_" + hashlib.sha1(key.encode()).hexdigest()[:20]


def find_node(name: str):
    """The curve node called ``name``, or None."""
    tree = bpy.data.node_groups.get(NODE_TREE)
    return tree.nodes.get(name) if tree is not None else None


def request(obj: bpy.types.Object, path: str, order: int):
    """Called while drawing: marks a curve as shown and returns its node if it exists."""
    name = binding_name(obj.name, path, order)
    binding = STATE.bindings.get(name)
    if binding is None:
        STATE.bindings[name] = CurveBinding(obj.name, path, order)
    else:
        binding.seen = time.monotonic()
    return find_node(name)


def binding_range(name: str) -> tuple[float, float] | None:
    """Value range shown by the widget of a binding, once it exists."""
    binding = STATE.bindings.get(name)
    return (binding.low, binding.high) if binding is not None and binding.last else None


def _create_node(name: str, label: str):
    tree = bpy.data.node_groups.get(NODE_TREE)
    if tree is None:
        tree = bpy.data.node_groups.new(NODE_TREE, "ShaderNodeTree")
    node = tree.nodes.new("ShaderNodeFloatCurve")
    node.name = name
    node.label = label[:63]
    return node


def read_node(node, binding: CurveBinding):
    """Species curve shown by a node."""
    points = [(p.location[0], p.location[1], p.handle_type) for p in node.mapping.curves[0].points]
    return curvemap.from_widget(points, binding.low, binding.high)


def write_node(node, curve, binding: CurveBinding) -> None:
    """Shows ``curve`` in a node."""
    mapping = node.mapping
    mapping.use_clip = True
    mapping.clip_min_x, mapping.clip_max_x = 0.0, 1.0
    mapping.clip_min_y, mapping.clip_max_y = 0.0, 1.0
    target = curvemap.to_widget(curve, binding.low, binding.high)
    points = mapping.curves[0].points
    while len(points) > max(2, len(target)):
        points.remove(points[len(points) - 1])
    while len(points) < len(target):
        points.new(0.5, 0.5)
    for point, (x, y, handle) in zip(points, target, strict=False):
        point.location = (x, y)
        point.handle_type = handle
    mapping.update()
    mapping.reset_view()


def tag_redraw() -> None:
    """Redraws the 3D view sidebars, where the widgets are."""
    for window in bpy.context.window_manager.windows:
        for area in window.screen.areas:
            if area.type == "VIEW_3D":
                area.tag_redraw()


def _sync_one(name: str, binding: CurveBinding) -> bool:
    """Syncs one binding; returns whether the panels need a redraw."""
    obj = bpy.data.objects.get(binding.object_name)
    spec = STATE.specs.get(binding.path)
    if not plant.is_plant(obj) or spec is None:
        STATE.bindings.pop(name, None)
        return False
    settings = obj.arbrume
    value_curve = entry_points(entry_at(props.current_value(settings, spec), binding.order))
    if value_curve is None:
        return False
    node = find_node(name)
    if node is None or binding.last is None:
        binding.low, binding.high = curvemap.value_range(value_curve, spec.soft_min, spec.soft_max)
        node = node or _create_node(name, f"{obj.name} {binding.path} {binding.order}")
        write_node(node, value_curve, binding)
        binding.last = read_node(node, binding)
        return True
    node_curve = read_node(node, binding)
    if not node_curve.close_to(binding.last):
        binding.last = node_curve
        if plant.is_editable(obj):
            props.write_curve_entry(settings, spec, points_entry(node_curve), binding.order)
        return False
    if not value_curve.close_to(binding.last):
        write_node(node, value_curve, binding)
        binding.last = read_node(node, binding)
        return True
    return False


def sync_bindings() -> float:
    """Timer: creates and syncs the nodes of the curves shown recently."""
    now = time.monotonic()
    redraw = False
    for name, binding in list(STATE.bindings.items()):
        if now - binding.seen > STALE_SECONDS:
            STATE.bindings.pop(name, None)
            continue
        try:
            redraw |= _sync_one(name, binding)
        except (ReferenceError, RuntimeError, AttributeError, ValueError) as error:
            STATE.bindings.pop(name, None)
            status_of(binding.object_name).error = f"Curve editor: {error}"
    if redraw:
        tag_redraw()
    return SYNC_INTERVAL


def register() -> None:
    """Starts the sync timer."""
    if not bpy.app.timers.is_registered(sync_bindings):
        bpy.app.timers.register(sync_bindings, first_interval=SYNC_INTERVAL, persistent=True)


def unregister() -> None:
    """Stops the sync timer and removes the private node group."""
    if bpy.app.timers.is_registered(sync_bindings):
        bpy.app.timers.unregister(sync_bindings)
    tree = bpy.data.node_groups.get(NODE_TREE)
    if tree is not None and tree.users == 0:
        bpy.data.node_groups.remove(tree)
