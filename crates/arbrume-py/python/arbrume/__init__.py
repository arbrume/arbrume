"""Arbrume: procedural trees and plants, generated in Rust.

>>> import arbrume
>>> tree = arbrume.generate("shrub", seed=7, age=4)
>>> tree.parts[0].vertices.shape[1]
3

Mesh, skeleton and organ arrays are read-only NumPy views of the Rust memory (no copy);
they keep the tree data alive on their own. See ``docs/python.md`` in the repository.
"""

from arbrume._arbrume import (
    ORGAN_KINDS,
    Buffer,
    MeshPart,
    Organs,
    Skeleton,
    Tree,
    VoxelMask,
    __version__,
    distance,
    fit,
    generate,
    generate_many,
    generate_stand,
    growth_frames,
    install_pack,
    load_pack,
    packs,
    packs_dir,
    plant_seed,
    preset_toml,
    presets,
    remove_pack,
    voxel_mask,
)
from arbrume.forest import INSTANCE_DTYPE, Forest, forest

__all__ = [
    "INSTANCE_DTYPE",
    "ORGAN_KINDS",
    "Buffer",
    "Forest",
    "MeshPart",
    "Organs",
    "Skeleton",
    "Tree",
    "VoxelMask",
    "__version__",
    "distance",
    "fit",
    "forest",
    "generate",
    "generate_many",
    "generate_stand",
    "growth_frames",
    "install_pack",
    "load_pack",
    "packs",
    "packs_dir",
    "plant_seed",
    "preset_toml",
    "presets",
    "remove_pack",
    "voxel_mask",
]
