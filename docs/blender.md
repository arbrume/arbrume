# Blender extension

The extension in `blender/arbrume/` adds Arbrume plants to Blender 4.2 LTS and later. It
is a [Blender extension](https://docs.blender.org/manual/en/latest/advanced/extensions/index.html)
that bundles the `arbrume` wheel ([Python bindings](python.md)) and builds meshes from its
arrays.

## Install

Download the zip of your platform (`arbrume-<version>-windows_x64.zip`,
`linux_x64`, `linux_arm64`, `macos_x64`, `macos_arm64`) and install it with
*Edit > Preferences > Get Extensions > Install from Disk*, or drag the zip into Blender.
The zip carries the wheel of its platform; Blender installs it into the extension's own
site-packages. NumPy is Blender's own.

## Usage

*Add > Mesh > Arbrume Plant* adds a plant at the 3D cursor; its redo panel picks the
species preset, seed, age (by default the oldest age suggested by the species), scale and
quality. Everything else is in the sidebar of the 3D view (`N`), tab **Arbrume**, for the
active plant:

- **Species, seed, age, scale, quality.** Every change regenerates the plant at once
  (*Live update*); turn it off for very large plants and press *Regenerate*. The age slider
  scrubs growth: the plant is generated again at each age. The panel shows the triangle
  count and the time spent generating and uploading the mesh, or the generator's error.
- **Species parameters.** One sub-panel per table of the species (Architecture, Growth,
  Geometry, Foliage, Bark), with collapsible groups. Numbers are sliders, choices are
  menus, colours are colour pickers. Each parameter has a reset button, enabled when the
  plant changes it; *Reset All Parameters* drops every change.
- **Curves.** Curve parameters (axis length, branching angle, branch probability,
  internode length, leaf size along the axis and the others) are edited per branching
  order, chosen with *Curves of order*. A constant curve shows as a slider; the curve
  button turns it into a curve editor, the flat button back into a constant. The editor
  shows the curve normalized between the two values printed under it (Blender's curve
  widget cannot show angles in degrees directly). Vector handles are linear segments,
  automatic handles the smooth interpolation of the generator.
- **Batch and Files.** *Duplicate with New Seed*; *Scatter Variations* makes N plants in a
  grid next to the active one or at the other selected objects, with a number of distinct
  seeds: plants with the same seed share one mesh. *Create LODs* makes copies at
  decreasing quality in a new collection, named `_LOD0`, `_LOD1`, ...
  *Save Species File* writes the species of the plant with its changes as a standalone
  TOML file; *Load Species File* uses such a file for the active plant or a new one.

A plant is one mesh object. What it is made from is stored on the object (preset name or
species file text, seed, age, scale, quality and the changed parameters as JSON), so a
saved file reopens editable and a plant can be duplicated or relinked like any object.
Editing the mesh by hand is overwritten at the next regeneration.

Foliage is baked into the mesh by the generator; leaves as instances are not available
yet.

Preferences (*Edit > Preferences > Add-ons > Arbrume*): the quality of new plants, the
number of threads of batch generation, and whether to use the generator's normals.

## How it works

The bpy-free logic lives in `blender/arbrume/logic/` and is tested with pytest outside
Blender:

| Module | Role |
|---|---|
| `values` | overrides (dotted paths, applied as the generator applies them), `f32`-exact number text, curve conversions between species values and control points |
| `schema` | the parameter list, found from the data: the full species of every preset (`tree.metadata["species"]`) gives the fields; the generator, asked to accept a test override, says which fields are curves, which curves take one curve per order, and which strings an enumeration accepts |
| `uimeta` | labels, groups and slider ranges of known parameters; a parameter missing here still gets a field |
| `species` | species sources (preset or TOML text) and caches of the full species |
| `toml_out` | writing a species as TOML that reads back as the same species |
| `meshdata` | one set of flat arrays for the whole tree: parts concatenated, Y up turned to Z up, per-corner UVs, material index per face |
| `curvemap` | species curves to and from the points of a curve widget |

Since the parameter panels are built from the species data, a parameter added to the
species schema appears in Blender without a change to the extension.

The parameter properties (`Object.arbrume.params`) are generated at registration with
`get` and `set` functions: they read the effective species and write the overrides, and
store nothing themselves, so the overrides text is the only state. Curve widgets need a
curve owned by a datablock: each shown curve gets a Float Curve node in a private node
group, `.Arbrume Curves`, which is not saved. A timer syncs the nodes and the overrides
both ways.

Meshes are rebuilt in place: `clear_geometry`, then element counts with `add` and the data
through the generic attribute API (`position`, `.corner_vert`, `material_index`, the UV
map, the colour attribute `Col`, and the generator normals as the `custom_normal`
attribute where Blender reads it as free normals, else
`normals_split_custom_set_from_vertices`). The attribute API copies each buffer in one
call. Bark and leaves are two material slots of one mesh, which keeps a regeneration to
one upload and lets scattered copies share one datablock. Materials are created once per
species and material values: a Principled BSDF whose base colour is the material colour
multiplied by the colour attribute.

## Building the extension

```sh
uv run blender/build_extension.py                          # wheel of this machine
uv run blender/build_extension.py --wheel wheels/*.whl     # wheels built elsewhere
BLENDER=/path/to/blender uv run blender/build_extension.py # build and validate with Blender
```

The script collects the wheels into `blender/arbrume/wheels/` (building the wheel of this
machine with maturin unless wheels are given), groups them by Blender platform, stages
one copy of the package per platform with only that wheel and the manifest's `platforms`
and `wheels` filled in, and zips it into `blender/dist/`. With Blender it uses
`blender --command extension build` and `validate`; without, it writes a zip of the same
layout (manifest at the root). The wheels are `abi3`, so one wheel per platform serves
the Python of every Blender from 4.2 on. The `Wheels` workflow builds the wheels of all
platforms.

## Tests

```sh
uv run --no-project --with pytest --with <wheel> pytest blender/tests
uv run blender/tests/run_in_blender.py --blender <blender> --zip blender/dist/<zip> --out <dir>
uv run blender/tests/run_in_blender.py --ui --blender <blender> --zip <zip> --out <dir>
```

The first runs the bpy-free tests (CI runs them with ruff on `blender/`). The second
installs the zip into a temporary Blender user directory and runs Blender in background:
one plant per preset with vertex and triangle counts checked against `tree.stats`, shading
normals checked against the generator's, property and parameter changes, a curve edited
through its node, an age scrub, every operator, saving and reopening the file, disabling
and enabling the extension, upload timings and two EEVEE stills. The third opens a window,
draws the panels and saves screenshots of the sidebar.
