"""Species packs in Blender: the Packs box of the preferences, its operators, and the pack
species of the species menu with their thumbnails.

Packs live in the pack directory of the ``arbrume`` wheel (``arbrume.packs_dir()``: the
``ARBRUME_PACKS_DIR`` environment variable, or the user data directory), shared with the
command line. :func:`refresh` reads them again and rebuilds the menu; it runs at
registration and after every install or removal. Thumbnails are loaded into a preview
collection, so the menu shows them as icons. Image textures of a pack need nothing here:
the generator puts them on the materials as it does procedural textures.
"""

from __future__ import annotations

from typing import ClassVar

import bpy
import bpy.utils.previews
from bpy.props import StringProperty
from bpy_extras.io_utils import ImportHelper

import arbrume

from . import props
from .logic import packs as pack_logic
from .state import STATE

_PREVIEWS: dict[str, object] = {}


def _previews():
    """The preview collection of pack thumbnails, created on first use."""
    if "thumbnails" not in _PREVIEWS:
        _PREVIEWS["thumbnails"] = bpy.utils.previews.new()
    return _PREVIEWS["thumbnails"]


def refresh() -> None:
    """Reads the installed packs again and rebuilds the species menu and its icons."""
    try:
        STATE.packs = list(arbrume.packs())
        STATE.pack_error = ""
    except (OSError, ValueError) as error:
        STATE.packs = []
        STATE.pack_error = str(error)
    STATE.pack_groups = pack_logic.groups(STATE.packs)
    previews = _previews()
    previews.clear()
    STATE.pack_icons = {}
    for group in STATE.pack_groups:
        for entry in group.species:
            if entry.thumbnail:
                icon = previews.load(entry.id, entry.thumbnail, "IMAGE")
                STATE.pack_icons[entry.id] = icon.icon_id
    props.clear_enum_cache()


def release() -> None:
    """Frees the preview collection, before the extension is unregistered."""
    for collection in _PREVIEWS.values():
        bpy.utils.previews.remove(collection)
    _PREVIEWS.clear()
    STATE.packs, STATE.pack_groups, STATE.pack_icons = [], [], {}


class ARBRUME_OT_install_pack(bpy.types.Operator, ImportHelper):
    """Validate a species pack (.arbpack) and install it into the pack directory"""

    bl_idname = "arbrume.install_pack"
    bl_label = "Install Pack"

    filename_ext = ".arbpack"
    filter_glob: StringProperty(default="*.arbpack", options={"HIDDEN"})

    def execute(self, _context):
        try:
            pack = arbrume.install_pack(self.filepath)
        except (OSError, ValueError) as error:
            self.report({"ERROR"}, f"Cannot install {self.filepath}: {error}")
            return {"CANCELLED"}
        refresh()
        count = len(pack["species"])
        self.report({"INFO"}, f"Installed {pack['name']} {pack['version']}, {count} species")
        return {"FINISHED"}


class ARBRUME_OT_remove_pack(bpy.types.Operator):
    """Remove an installed species pack; plants of its species stop regenerating"""

    bl_idname = "arbrume.remove_pack"
    bl_label = "Remove Pack"

    pack_id: StringProperty(name="Pack", options={"HIDDEN"})

    def invoke(self, context, _event):
        return context.window_manager.invoke_confirm(self, None)

    def execute(self, _context):
        try:
            arbrume.remove_pack(self.pack_id)
        except (OSError, ValueError) as error:
            self.report({"ERROR"}, f"Cannot remove {self.pack_id}: {error}")
            return {"CANCELLED"}
        refresh()
        self.report({"INFO"}, f"Removed pack {self.pack_id}")
        return {"FINISHED"}


class ARBRUME_OT_refresh_packs(bpy.types.Operator):
    """Read the pack directory again"""

    bl_idname = "arbrume.refresh_packs"
    bl_label = "Refresh Packs"
    bl_options: ClassVar[set[str]] = set()

    def execute(self, _context):
        refresh()
        return {"FINISHED"}


def draw_preferences(layout) -> None:
    """The Packs box of the preferences: directory, install, and one row per pack."""
    box = layout.box()
    box.label(text="Species Packs", icon="PACKAGE")
    try:
        box.label(text=f"Directory: {arbrume.packs_dir()}")
    except (OSError, ValueError) as error:
        box.label(text=str(error), icon="ERROR")
    row = box.row()
    row.operator(ARBRUME_OT_install_pack.bl_idname, icon="IMPORT")
    row.operator(ARBRUME_OT_refresh_packs.bl_idname, icon="FILE_REFRESH")
    if STATE.pack_error:
        box.label(text=STATE.pack_error, icon="ERROR")
    if not STATE.packs:
        box.label(text="No pack installed")
    for pack in STATE.packs:
        row = box.row()
        species = len(pack["species"])
        row.label(text=f"{pack['name']} {pack['version']} ({species} species)")
        row.label(text=pack["content_licence"])
        op = row.operator(ARBRUME_OT_remove_pack.bl_idname, text="", icon="TRASH")
        op.pack_id = pack["id"]


CLASSES = (ARBRUME_OT_install_pack, ARBRUME_OT_remove_pack, ARBRUME_OT_refresh_packs)
