"""Forests: trees placed over a terrain, drawn from a small library of variants.

>>> import arbrume
>>> config = {
...     "terrain": {"size": [60.0, 60.0]},
...     "species": [{"species": "shrub", "density": 400.0, "spacing": 2.0}],
... }
>>> placed = arbrume.forest(config, grow=False)
>>> placed.positions.shape[1]
3

See ``docs/forest.md`` for the config keys. The placement is deterministic: it depends
on the config only, never on the number of threads.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Any

import numpy as np

from arbrume._arbrume import Tree, forest_build

INSTANCE_DTYPE = np.dtype(
    [
        ("position", "<f4", (3,)),
        ("rotation", "<f4", (4,)),
        ("scale", "<f4"),
        ("species", "<u2"),
        ("variant", "<u2"),
        ("tint", "u1", (4,)),
    ]
)
"""One record of the instance table (``docs/forest.md``, 40 bytes)."""


@dataclass(frozen=True)
class Forest:
    """A placed forest.

    ``instances`` is a structured array of :data:`INSTANCE_DTYPE`, one record per tree;
    the other array attributes are views of its fields. ``library[s][k]`` describes
    variant ``k`` of species ``s``: ``levels`` (one :class:`arbrume.Tree` per level of
    detail, the billboard last), ``kinds`` (``"mesh"`` or ``"billboard"``),
    ``max_screen_height``, ``seed``, ``height``, ``crown_radius`` and ``trunk_radius``;
    it is empty when the forest was placed without growing. ``coupled`` lists the trees
    of the coupled region as ``(instance index, Tree)``.
    """

    instances: np.ndarray
    names: list[str]
    library: list[list[dict[str, Any]]]
    terrain: dict[str, Any]
    report: dict[str, Any]
    coupled: list[tuple[int, Tree]]

    @property
    def positions(self) -> np.ndarray:
        """Base of each trunk, ``(n, 3)`` meters, Y up."""
        return self.instances["position"]

    @property
    def rotations(self) -> np.ndarray:
        """Unit quaternion ``(x, y, z, w)`` of each tree, ``(n, 4)``."""
        return self.instances["rotation"]

    @property
    def scales(self) -> np.ndarray:
        """Uniform scale of each tree, ``(n,)``."""
        return self.instances["scale"]

    @property
    def species(self) -> np.ndarray:
        """Species index of each tree (into ``names``), ``(n,)``."""
        return self.instances["species"]

    @property
    def variants(self) -> np.ndarray:
        """Variant index of each tree within its species, ``(n,)``."""
        return self.instances["variant"]

    @property
    def tints(self) -> np.ndarray:
        """Colour factor of each tree as floats, ``(n, 3)`` (1 is no change)."""
        return self.instances["tint"][:, :3].astype(np.float32) / 128.0


def forest(
    config: dict[str, Any] | str | os.PathLike,
    *,
    grow: bool = True,
    base: str | os.PathLike | None = None,
    out: str | os.PathLike | None = None,
    threads: int | None = None,
) -> Forest:
    """Places the forest of ``config`` and, with ``grow``, grows its variant library.

    ``config`` is a dict with the keys of ``forest.toml``, the TOML text of one, or the
    path of a file. Relative paths in it (heightmaps, masks, species files) start from
    ``base``, by default the directory of the file or the current directory. Without
    ``grow`` no variant is grown, so every species needs a ``spacing``. With ``out``,
    the files of ``arbrume forest`` are also written there. ``threads`` sets the worker
    threads (default: the global pool). Raises ``ValueError`` for an invalid config.
    """
    raw = forest_build(
        config if isinstance(config, dict) else os.fspath(config),
        grow=grow,
        base=None if base is None else os.fspath(base),
        out=None if out is None else os.fspath(out),
        threads=threads,
    )
    instances = np.frombuffer(raw["records"], dtype=INSTANCE_DTYPE, count=raw["count"])
    terrain = dict(raw["terrain"])
    columns, rows = terrain["samples"]
    terrain["heights"] = np.frombuffer(terrain["heights"], dtype="<f4").reshape(rows, columns)
    return Forest(
        instances=instances,
        names=list(raw["names"]),
        library=raw["variants"],
        terrain=terrain,
        report=raw["report"],
        coupled=list(raw["coupled"]),
    )
