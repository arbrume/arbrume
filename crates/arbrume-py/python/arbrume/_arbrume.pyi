"""Type stubs of the Rust extension module ``arbrume._arbrume``."""

import os
from collections.abc import Iterable, Sequence
from typing import Any, Literal, final

import numpy as np
import numpy.typing as npt

__version__: str
ORGAN_KINDS: tuple[str, ...]

_Species = str | os.PathLike[str]
_Float32 = npt.NDArray[np.float32]

def generate(
    species: _Species,
    seed: int = 0,
    age: int | None = None,
    *,
    scale: float = 1.0,
    quality: float = 1.0,
    textures: bool = False,
    junctions: Literal["tier-a", "tier-b"] = "tier-a",
    overrides: dict[str, Any] | None = None,
    **dotted_overrides: Any,
) -> Tree:
    """Generate one tree from a preset name, a species file or TOML text."""

def generate_many(
    species: _Species | Sequence[dict[str, Any]],
    seeds: Iterable[int] | None = None,
    age: int | None = None,
    *,
    threads: int | None = None,
    scale: float = 1.0,
    quality: float = 1.0,
    textures: bool = False,
    junctions: Literal["tier-a", "tier-b"] = "tier-a",
    overrides: dict[str, Any] | None = None,
    **dotted_overrides: Any,
) -> list[Tree]:
    """Generate many trees in parallel without the GIL, results in input order."""

def presets() -> list[str]:
    """Names of the built-in species presets."""

def preset_toml(name: str) -> str:
    """TOML text of a built-in preset."""

def plant_seed(seed: int, index: int) -> int:
    """Seed of plant ``index`` of a dataset with base seed ``seed``."""

@final
class Buffer:
    """Read-only buffer-protocol view of one array of a tree."""

    @property
    def shape(self) -> list[int]: ...
    @property
    def format(self) -> str: ...
    def __len__(self) -> int: ...
    def __buffer__(self, flags: int, /) -> memoryview: ...

@final
class MeshPart:
    """One drawable part of a tree mesh: buffers and a material."""

    @property
    def kind(self) -> str: ...
    @property
    def material(self) -> dict[str, Any]: ...
    @property
    def vertices(self) -> _Float32: ...
    @property
    def normals(self) -> _Float32: ...
    @property
    def uvs(self) -> _Float32: ...
    @property
    def colors(self) -> _Float32: ...
    @property
    def indices(self) -> npt.NDArray[np.uint32]: ...
    @property
    def vertex_count(self) -> int: ...
    @property
    def triangle_count(self) -> int: ...
    def buffer(
        self, name: Literal["vertices", "normals", "uvs", "colors", "indices"]
    ) -> Buffer: ...

@final
class Skeleton:
    """One row per node (metamer) of the plant graph, parents before children."""

    @property
    def positions(self) -> _Float32: ...
    @property
    def radii(self) -> _Float32: ...
    @property
    def parents(self) -> npt.NDArray[np.int32]: ...
    @property
    def orders(self) -> npt.NDArray[np.uint8]: ...
    @property
    def births(self) -> npt.NDArray[np.uint16]: ...
    @property
    def chains(self) -> npt.NDArray[np.int32]: ...
    @property
    def shed(self) -> npt.NDArray[np.bool_]: ...
    def buffer(self, name: str) -> Buffer: ...
    def __len__(self) -> int: ...

@final
class Organs:
    """One row per organ site (leaf, twig, flower, fruit)."""

    @property
    def nodes(self) -> npt.NDArray[np.int32]: ...
    @property
    def kinds(self) -> npt.NDArray[np.uint8]: ...
    @property
    def directions(self) -> _Float32: ...
    @property
    def normals(self) -> _Float32: ...
    @property
    def scales(self) -> _Float32: ...
    @property
    def births(self) -> npt.NDArray[np.uint16]: ...
    def buffer(self, name: str) -> Buffer: ...
    def __len__(self) -> int: ...

@final
class Tree:
    """A generated tree: mesh parts, skeleton, organ sites, statistics and metadata."""

    @property
    def parts(self) -> list[MeshPart]: ...
    @property
    def skeleton(self) -> Skeleton: ...
    @property
    def organs(self) -> Organs: ...
    @property
    def stats(self) -> dict[str, Any]: ...
    @property
    def metadata(self) -> dict[str, Any]: ...
    @property
    def seed(self) -> int: ...
    @property
    def age(self) -> int: ...
    @property
    def scale(self) -> float: ...
    @property
    def species_name(self) -> str: ...
    def to_glb(self, name: str = "tree", extras: dict[str, Any] | None = None) -> bytes: ...
    def save_glb(
        self,
        path: str | os.PathLike[str],
        name: str | None = None,
        extras: dict[str, Any] | None = None,
    ) -> None: ...
