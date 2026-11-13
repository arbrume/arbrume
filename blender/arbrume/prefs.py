"""Preferences of the extension and its About box."""

import tomllib
from pathlib import Path

import bpy
from bpy.props import BoolProperty, FloatProperty, IntProperty

import arbrume

from .state import STATE

LICENSE = "GPL-3.0-or-later"
MANIFEST = Path(__file__).with_name("blender_manifest.toml")


def extension_version() -> str:
    """Version of the extension, from its manifest."""
    try:
        return str(tomllib.loads(MANIFEST.read_text(encoding="utf-8")).get("version", ""))
    except (OSError, tomllib.TOMLDecodeError):
        return ""


class ARBRUME_AP_preferences(bpy.types.AddonPreferences):
    """Default quality, threads and normals of new plants"""

    bl_idname = __package__

    default_quality: FloatProperty(
        name="Default Quality",
        description="Quality of new plants, 1 is full detail",
        default=1.0,
        min=0.01,
        max=4.0,
        soft_max=1.0,
    )
    threads: IntProperty(
        name="Threads",
        description="Worker threads of batch generation, 0 for all cores",
        default=0,
        min=0,
        max=256,
    )
    custom_normals: BoolProperty(
        name="Generator Normals",
        description="Use the smooth normals computed by the generator (seamless bark)",
        default=True,
    )

    def draw(self, _context):
        layout = self.layout
        col = layout.column()
        col.use_property_split = True
        col.prop(self, "default_quality")
        col.prop(self, "threads")
        col.prop(self, "custom_normals")
        box = layout.box()
        box.label(text="About Arbrume", icon="INFO")
        box.label(text=f"Extension {extension_version()}, generator {arbrume.__version__}")
        box.label(text=f"{len(STATE.presets)} species presets")
        box.label(text=f"Free software, licensed {LICENSE}")
        box.label(text="Procedural trees and plants generated in Rust")
