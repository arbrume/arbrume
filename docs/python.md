# Python bindings

The `arbrume` Python package wraps the `arbrume-gen` pipeline with PyO3. It generates
trees from Python and hands their meshes and skeletons to NumPy without copying them.

## Install and build

The wheels use the stable ABI (`abi3`) from Python 3.11 on: one wheel per platform loads
in CPython 3.11, 3.12, 3.13 and later, which covers Blender 4.2 LTS and every later
Blender. NumPy is the only runtime dependency. The free-threaded builds of CPython
cannot load `abi3` wheels.

Build from the repository with [maturin](https://www.maturin.rs) (1.9.4 or later):

```sh
cd crates/arbrume-py
uvx maturin build --release            # target/wheels/arbrume-<version>-cp311-abi3-<platform>.whl
uv venv && uvx maturin develop --release --uv   # editable install into .venv
```

The `Wheels` workflow (`.github/workflows/wheels.yml`, run by hand or on a tag) builds
the wheels for Linux x86_64 and aarch64 (manylinux), Windows x86_64 and macOS x86_64
and arm64, and uploads them as artifacts.

The tests are in `crates/arbrume-py/tests` (pytest). Set `ARBRUME_CLI` to the path of
the `arbrume` binary to also check that a tree exported from Python is byte-identical to
the file the command line writes for the same inputs.

## Generating trees

```python
import arbrume
import numpy as np

arbrume.presets()  # ['broadleaf', 'conifer', 'shrub']
tree = arbrume.generate("broadleaf", seed=42, age=20, quality=0.5)
tree = arbrume.generate("oak.toml", seed=1)  # a species file
tree = arbrume.generate(arbrume.preset_toml("shrub"), seed=1)  # TOML text
tree = arbrume.generate("broadleaf", seed=1, **{"foliage.length": 0.2})
tree = arbrume.generate("broadleaf", seed=1, overrides={"growth.gnarl": 5.0})

for part in tree.parts:  # bark first, then foliage
    print(part.kind, part.material["name"], part.vertices.shape, part.indices.shape)

bark = tree.parts[0]
positions = bark.vertices  # (n, 3) float32, meters, Y up
triangles = bark.indices  # (m, 3) uint32
area = (
    0.5
    * np.linalg.norm(
        np.cross(
            positions[triangles[:, 1]] - positions[triangles[:, 0]],
            positions[triangles[:, 2]] - positions[triangles[:, 0]],
        ),
        axis=1,
    ).sum()
)

skeleton = tree.skeleton  # one row per node, parents before children
tips = np.setdiff1d(np.arange(len(skeleton)), skeleton.parents)
trunk = skeleton.positions[skeleton.orders == 0]

leaves = tree.organs.kinds == arbrume.ORGAN_KINDS.index("leaf")
leaf_nodes = tree.organs.nodes[leaves]

tree.save_glb("broadleaf_42.glb")  # root node named after the file stem
data = tree.to_glb()  # the same file as bytes
tree.stats["triangles"], tree.metadata["seed"]
```

`species` is a preset name, else a path to an existing species file (`str` or
`pathlib.Path`), else TOML text. Overrides are dotted paths into the species file, as in
the `overrides` of a batch config file (see [](cli.md)); since dotted names are not
valid Python keywords, pass them with `**{...}` or as `overrides={...}`. `age` defaults
to 10 cycles, `scale` and `quality` to 1. The mesh is seeded with the plant seed, as in
the command line, so `generate(s, seed=arbrume.plant_seed(base, i), ...)` gives plant
`i` of `arbrume gen --seed base`. With `textures=True` the materials of the parts carry
the procedural textures of the species ([Textures](textures.md)) as PNG bytes in
`part.material` (`base_color_texture`, `normal_texture`, `roughness_texture`) and the
leaves are cards cut by the alpha of the texture; the textures of each species are
synthesized once and kept for the process (the cache is emptied beyond 32 species).
With `junctions="tier-b"` the branches are stitched into their parents and the bark of
each stem is one closed surface once vertices of equal position are welded
([Junctions](junctions.md)); the default is `"tier-a"`.

Errors raise `ValueError` with the message of the Rust error (invalid species file,
unknown or invalid override, invalid scale, quality, seed or age),
`FileNotFoundError` for a missing species file and `TypeError` for an argument of the
wrong type.

### Batches

```python
trees = arbrume.generate_many("conifer", seeds=range(1000), age=15, threads=8)
trees = arbrume.generate_many(
    [
        {"species": "broadleaf", "seed": 1, "age": 20},
        {"species": "shrub", "seed": 2, "overrides": {"foliage.length": 0.05}},
    ],
    quality=0.5,  # default for the jobs that set none
)
```

`generate_many` releases the GIL and runs the batch on a `rayon` pool (`threads`
workers, default: all cores or `RAYON_NUM_THREADS`). The trees come back in input order
and are identical to calling `generate` on each job, whatever the number of threads.
Every tree of the batch is held in memory; split very large datasets into chunks, or
use the command line. `generate` releases the GIL too, so Python threads may call it
concurrently.

## Arrays without copies

| Object | Array | Shape | dtype |
|---|---|---|---|
| `MeshPart` | `vertices`, `normals` | `(n, 3)` | float32 |
| | `uvs` | `(n, 2)` | float32 |
| | `colors` (linear RGBA) | `(n, 4)` | float32 |
| | `indices` (triangles) | `(m, 3)` | uint32 |
| `Skeleton` | `positions` | `(k, 3)` | float32 |
| | `radii` | `(k,)` | float32 |
| | `parents` (-1 for a root) | `(k,)` | int32 |
| | `orders`, `births`, `chains`, `shed` | `(k,)` | uint8, uint16, int32, bool |
| `Organs` | `nodes`, `kinds` | `(s,)` | int32, uint8 |
| | `directions`, `normals` | `(s, 3)` | float32 |
| | `scales`, `births` | `(s,)` | float32, uint16 |

`normals`, `uvs` and `colors` have either one row per vertex or none. The arrays are
read-only views of the Rust buffers: each property returns a new `ndarray` over the
same memory (`np.shares_memory(part.vertices, part.vertices)` is true), whose `base` is
a `memoryview` of an `arbrume.Buffer`. They stay valid after the `Tree` object is
deleted, since every view holds a reference to the data. Use `array.copy()` for a
writable copy.

The bridge is the Python buffer protocol, which is part of the stable ABI since Python
3.11: the extension implements `bf_getbuffer`, and `numpy.asarray` wraps the buffer
without copying. The `numpy` Rust crate was not used because it needs the full,
version-specific C API. `part.buffer("vertices")` returns the raw `Buffer`, usable as
`memoryview(...)` without NumPy.

## Fitting to a skeleton

`arbrume.distance(a, b, *, min_radius=0.0, step=None, smooth=0, up="y")` compares two
tree skeletons and returns the terms of the distance as a dict; `arbrume.fit(target,
species, **settings)` fits the parameters of `species` to `target` and returns the fitted
species file (`toml`), the fitted `age`, `scale` and `factors`, and the `report`. A
skeleton is a `Tree`, a path (`.ann.npz`, `.glb`, `.csv`, `.json`) or a dict of
`position`, `radius` and `parent` arrays. Settings, method and file formats are those of
`arbrume fit` ([Parameter fitting](fitting.md)); both run without the GIL, and `fit`
gives the same result whatever `threads`.

```python
result = arbrume.fit("scan.csv", "oak_like", up="z", min_radius=0.02, checkpoint="oak.json")
tree = arbrume.generate(result["toml"], seed=1, age=result["age"], scale=result["scale"])
arbrume.distance("scan.csv", tree, up="z", min_radius=0.02)["total"]
```

## Species packs

Species of packs ([Species packs](packs.md)) are named `"pack_id/name"`, which
`generate` and `generate_many` accept wherever they accept a preset name. They are found
among the packs given to `load_pack` in this process, then among the installed packs of
the pack directory (`ARBRUME_PACKS_DIR`, or the user data directory).

```python
# a pack directory or a .arbpack file, kept for this process
pack = arbrume.load_pack("my_trees")
ids = [s["id"] for s in pack["species"]]  # ['my_trees/field_oak', ...]
tree = arbrume.generate("my_trees/field_oak", seed=1, textures=True)

# validated, then installed into arbrume.packs_dir()
arbrume.install_pack("my_trees-0.1.0.arbpack")
installed = arbrume.packs()
arbrume.remove_pack("my_trees")
```

A pack is described by a dict: `id`, `name`, `version`, `authors`, `description`,
`content_licence`, `licence_file`, `min_schema_version`, `path`, `archive` and `species`,
a list of dicts with `id`, `name`, `display_name`, `category`, `thumbnail` (a file path or
`None`) and `preview`. `load_pack` checks the structure of a pack (allowed files, hashes,
`pack.toml`); `install_pack` validates it fully, species grown included, and raises
`ValueError` listing every error. A missing pack or species raises `FileNotFoundError`.
Image textures of a pack are used with `textures=True`. The texture paths of a species
file given by path are relative to its directory.

## Blender

The Blender extension will build meshes from these arrays with `foreach_set`, which
reads any buffer of the right type directly, so the data goes from Rust to Blender with
no Python loop and no intermediate list:

```python
import bpy
import numpy as np

part = tree.parts[0]
mesh = bpy.data.meshes.new(part.material["name"])
mesh.vertices.add(part.vertex_count)
mesh.vertices.foreach_set("co", part.vertices.ravel())
loops = part.indices.ravel()
mesh.loops.add(len(loops))
mesh.loops.foreach_set("vertex_index", loops.astype(np.int32))
mesh.polygons.add(part.triangle_count)
mesh.polygons.foreach_set("loop_start", np.arange(0, len(loops), 3, dtype=np.int32))
mesh.update()
uv = mesh.uv_layers.new()
uv.data.foreach_set("uv", part.uvs[loops].ravel())
```

Arbrume coordinates are Y up (glTF); Blender is Z up, so the extension rotates the
object by 90 degrees about X (or swaps the axes in NumPy). `ravel()` of a C-contiguous
array is a view, not a copy; `astype(np.int32)` and the per-loop UV gather do copy,
because Blender wants `int32` indices and per-loop UVs.
