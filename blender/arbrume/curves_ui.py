"""Curve widgets for curve parameters, kept in sync with the overrides.

Blender only draws a curve widget (``layout.template_curve_mapping``) for a curve that
belongs to a datablock, so each edited curve gets a Float Curve node in a private node
group (``.Arbrume Curves``, hidden from lists and not kept when unused). Panels cannot
change data while drawing: they only record which curves are shown, and a timer creates
the nodes and syncs both ways. A node edited by the user writes its points to the
override of its plant; an override changed elsewhere (undo, reset, a species file) is
written back to the node.

Undo. Each node keeps, as a custom property, the curve it showed when it last agreed with
the overrides (with its plant, parameter, order and value range). The property is node
data, so undo and redo restore it with the node and the overrides, and they tell which
side changed after the last sync: points that differ from the record were edited in the
widget, an override that differs from it was changed elsewhere. Blender's widget pushes
its own undo step when a drag ends, often before the timer has written the overrides; the
``undo_post`` and ``redo_post`` handlers complete such a step by writing the edited
points to the overrides, which regenerates the plant, and they never push a step. For
edits that Blender records without a step (some widget buttons, scripts), the timer pushes
an ``Edit Arbrume Curve`` step once the points have stopped changing, unless an undo step
was pushed since the edit began (read from ``WindowManager.undo_stack`` where Blender has
it; without it the timer always pushes).
"""

from __future__ import annotations

import hashlib
import json
import time
from dataclasses import dataclass

import bpy
from bpy.app.handlers import persistent

from . import plant, props
from .logic import curvemap
from .logic.values import CurvePoints, entry_at, entry_points, points_entry
from .state import STATE, CurveBinding, status_of

NODE_TREE = ".Arbrume Curves"
SYNC_INTERVAL = 0.2
STALE_SECONDS = 10.0
SYNCED_KEY = "arbrume_synced"
UNDO_MESSAGE = "Edit Arbrume Curve"
SYNC_ERRORS = (ReferenceError, RuntimeError, AttributeError, ValueError)


@dataclass
class Synced:
    """What a node showed when it last agreed with the overrides of its plant."""

    object_name: str
    path: str
    order: int
    low: float
    high: float
    curve: CurvePoints


def binding_name(object_name: str, path: str, order: int) -> str:
    """Name of the node holding one curve of one plant."""
    key = f"{object_name}\x00{path}\x00{order}"
    return "curve_" + hashlib.sha1(key.encode()).hexdigest()[:20]


def find_node(name: str):
    """The curve node called ``name``, or None."""
    tree = bpy.data.node_groups.get(NODE_TREE)
    return tree.nodes.get(name) if tree is not None else None


def synced_of(node) -> Synced | None:
    """The sync record of a node, or None when it has none."""
    text = node.get(SYNCED_KEY) if node is not None else None
    if not isinstance(text, str):
        return None
    try:
        data = json.loads(text)
        curve = entry_points(data["curve"])
        if curve is None:
            return None
        return Synced(
            str(data["object"]),
            str(data["path"]),
            int(data["order"]),
            float(data["low"]),
            float(data["high"]),
            curve,
        )
    except (ValueError, KeyError, TypeError):
        return None


def _record(node, synced: Synced) -> None:
    data = {
        "object": synced.object_name,
        "path": synced.path,
        "order": synced.order,
        "low": synced.low,
        "high": synced.high,
        "curve": points_entry(synced.curve),
    }
    node[SYNCED_KEY] = json.dumps(data)


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
    """Value range shown by the widget of a binding, once its node exists."""
    synced = synced_of(find_node(name))
    return (synced.low, synced.high) if synced is not None else None


def _create_node(name: str, label: str):
    tree = bpy.data.node_groups.get(NODE_TREE)
    if tree is None:
        tree = bpy.data.node_groups.new(NODE_TREE, "ShaderNodeTree")
    node = tree.nodes.new("ShaderNodeFloatCurve")
    node.name = name
    node.label = label[:63]
    return node


def read_node(node, low: float, high: float) -> CurvePoints:
    """Species curve shown by a node whose widget spans ``low`` to ``high``."""
    points = [(p.location[0], p.location[1], p.handle_type) for p in node.mapping.curves[0].points]
    return curvemap.from_widget(points, low, high)


def write_node(node, curve: CurvePoints, low: float, high: float) -> None:
    """Shows ``curve`` in a node whose widget spans ``low`` to ``high``."""
    mapping = node.mapping
    mapping.use_clip = True
    mapping.clip_min_x, mapping.clip_max_x = 0.0, 1.0
    mapping.clip_min_y, mapping.clip_max_y = 0.0, 1.0
    target = curvemap.to_widget(curve, low, high)
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


def _show(node, curve: CurvePoints, synced: Synced) -> None:
    """Writes ``curve`` to a node and records it as synced."""
    write_node(node, curve, synced.low, synced.high)
    synced.curve = read_node(node, synced.low, synced.high)
    _record(node, synced)


def tag_redraw() -> None:
    """Redraws the 3D view sidebars, where the widgets are."""
    for window in bpy.context.window_manager.windows:
        for area in window.screen.areas:
            if area.type == "VIEW_3D":
                area.tag_redraw()


def _plant_curve(obj, spec, order: int) -> CurvePoints | None:
    """Curve of one order of a curve parameter of a plant, or None for a constant."""
    return entry_points(entry_at(props.current_value(obj.arbrume, spec), order))


def _sync_node(node, synced: Synced, obj, spec) -> tuple[bool, bool]:
    """Syncs an existing node with its plant: points edited since the record win, else
    the overrides. Returns whether the node changed and whether the overrides changed."""
    value_curve = _plant_curve(obj, spec, synced.order)
    if value_curve is None:
        return False, False
    node_curve = read_node(node, synced.low, synced.high)
    if not node_curve.close_to(synced.curve):
        synced.curve = node_curve
        _record(node, synced)
        if not plant.is_editable(obj):
            return False, False
        before = obj.arbrume.overrides
        props.write_curve_entry(obj.arbrume, spec, points_entry(node_curve), synced.order)
        return False, obj.arbrume.overrides != before
    if not value_curve.close_to(synced.curve):
        _show(node, value_curve, synced)
        return True, False
    return False, False


def _sync_binding(name: str, binding: CurveBinding) -> tuple[bool, bool]:
    """Creates or syncs the node of one binding; returns as :func:`_sync_node`."""
    obj = bpy.data.objects.get(binding.object_name)
    spec = STATE.specs.get(binding.path)
    if not plant.is_plant(obj) or spec is None:
        STATE.bindings.pop(name, None)
        return False, False
    node = find_node(name)
    synced = synced_of(node)
    if synced is not None and (synced.object_name, synced.path) == (obj.name, binding.path):
        return _sync_node(node, synced, obj, spec)
    value_curve = _plant_curve(obj, spec, binding.order)
    if value_curve is None:
        return False, False
    low, high = curvemap.value_range(value_curve, spec.soft_min, spec.soft_max)
    node = node or _create_node(name, f"{obj.name} {binding.path} {binding.order}")
    _show(node, value_curve, Synced(obj.name, binding.path, binding.order, low, high, value_curve))
    return True, False


def undo_marker() -> tuple | None:
    """Identifies the active undo step, or None where Blender does not expose its stack."""
    stack = getattr(bpy.context.window_manager, "undo_stack", None)
    if stack is None:
        return None
    active = stack.active
    return len(stack.steps), stack.active_index, active.as_pointer() if active else 0


def push_undo() -> None:
    """Records the current state as the undo step of a curve edit.

    Timers run without a window in their context, so the push runs in the first window
    when there is one (always, except in background mode).
    """
    windows = bpy.context.window_manager.windows
    if bpy.context.window is None and len(windows) > 0:
        with bpy.context.temp_override(window=windows[0]):
            bpy.ops.ed.undo_push(message=UNDO_MESSAGE)
    else:
        bpy.ops.ed.undo_push(message=UNDO_MESSAGE)


def _record_edit(edited: bool) -> None:
    """Pushes the undo step of a widget edit once its points have stopped changing,
    unless Blender pushed a step after the edit began."""
    if edited and not STATE.edit_pending:
        STATE.edit_pending = True
        STATE.edit_marker = STATE.undo_marker
    elif not edited and STATE.edit_pending:
        STATE.edit_pending = False
        marker = undo_marker()
        if marker is None or marker == STATE.edit_marker:
            try:
                push_undo()
            except RuntimeError as error:
                print("Arbrume: curve edit not recorded for undo:", error)
    STATE.undo_marker = undo_marker()


def sync_bindings() -> float:
    """Timer: creates and syncs the nodes of the curves shown recently."""
    now = time.monotonic()
    redraw = edited = False
    for name, binding in list(STATE.bindings.items()):
        if now - binding.seen > STALE_SECONDS:
            STATE.bindings.pop(name, None)
            continue
        try:
            node_changed, overrides_changed = _sync_binding(name, binding)
        except SYNC_ERRORS as error:
            STATE.bindings.pop(name, None)
            status_of(binding.object_name).error = f"Curve editor: {error}"
            continue
        redraw |= node_changed
        edited |= overrides_changed
    _record_edit(edited)
    if redraw:
        tag_redraw()
    return SYNC_INTERVAL


def sync_all_nodes() -> None:
    """Syncs every curve node with its plant, shown or not, without pushing undo steps."""
    tree = bpy.data.node_groups.get(NODE_TREE)
    for node in list(tree.nodes) if tree is not None else []:
        synced = synced_of(node)
        obj = bpy.data.objects.get(synced.object_name) if synced is not None else None
        spec = STATE.specs.get(synced.path) if synced is not None else None
        if not plant.is_plant(obj) or spec is None:
            continue
        try:
            _sync_node(node, synced, obj, spec)
        except SYNC_ERRORS as error:
            status_of(synced.object_name).error = f"Curve editor: {error}"


@persistent
def arbrume_after_undo(*_args) -> None:
    """Undo or redo: completes the restored step from the node records, no step pushed."""
    sync_all_nodes()
    STATE.edit_pending = False
    STATE.undo_marker = undo_marker()
    tag_redraw()


def _undo_handlers() -> list:
    return [bpy.app.handlers.undo_post, bpy.app.handlers.redo_post]


def _remove_undo_handlers() -> None:
    for handlers in _undo_handlers():
        for handler in [h for h in handlers if getattr(h, "__name__", "") == "arbrume_after_undo"]:
            handlers.remove(handler)


def register() -> None:
    """Starts the sync timer and adds the undo and redo handlers."""
    _remove_undo_handlers()
    for handlers in _undo_handlers():
        handlers.append(arbrume_after_undo)
    if not bpy.app.timers.is_registered(sync_bindings):
        bpy.app.timers.register(sync_bindings, first_interval=SYNC_INTERVAL, persistent=True)


def unregister() -> None:
    """Stops the sync timer, removes the handlers and the private node group."""
    _remove_undo_handlers()
    if bpy.app.timers.is_registered(sync_bindings):
        bpy.app.timers.unregister(sync_bindings)
    tree = bpy.data.node_groups.get(NODE_TREE)
    if tree is not None and tree.users == 0:
        bpy.data.node_groups.remove(tree)
