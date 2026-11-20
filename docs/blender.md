# Blender extension

The extension in `blender/arbrume/` adds Arbrume plants to Blender 4.2 LTS and later. It
is a [Blender extension](https://docs.blender.org/manual/en/latest/advanced/extensions/index.html)
that bundles the `arbrume` wheel ([Python bindings](python.md)) and builds meshes from its
arrays.

## Install

Download the zip of your platform (`arbrume-<version>-windows_x64.zip`,
`linux_x64`, `macos_x64`, `macos_arm64`) and install it with
*Edit > Preferences > Get Extensions > Install from Disk*, or drag the zip into Blender.
The zip carries the wheel of its platform; Blender installs it into the extension's own
site-packages. NumPy is Blender's own.

The manifest declares one permission, `files`: species TOML files are loaded and saved
through file browsers. The extension uses no network access.

## Usage

*Add > Mesh > Arbrume Plant* adds a plant at the 3D cursor; its redo panel picks the
species preset, seed, age (by default the oldest age suggested by the species), scale and
quality. Everything else is in the sidebar of the 3D view (`N`), tab **Arbrume**, for the
active plant:

- **Textures.** With *Textures* on (the default), the plant carries the procedural bark
  and leaf textures of its species ([Textures](textures.md)), synthesized once per
  species and kept by the extension for the session.
- **Junctions.** *Collar* (the default) starts each branch inside its parent; *Watertight*
  stitches it into its parent, so the bark of each stem is one closed surface
  ([Junctions](junctions.md)). The imported mesh keeps one vertex per side of each
  texture seam: before a Subdivision Surface modifier or a 3D print, run *Merge by
  Distance* with distance 0, which keeps the seams in the UV maps.
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
  automatic handles the smooth interpolation of the generator. A curve edit is one undo
  step: undo and redo bring back the curve and the plant together.
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

Undo of curve edits: each node stores, as a custom property, the curve it showed when it
last agreed with the overrides. Undo restores that record with the node and the
overrides, so after an undo or redo the extension can tell which side changed: points
that differ from the record were edited in the widget and are written to the overrides
(which regenerates the plant), otherwise the node is rewritten from the overrides. This
matters because Blender's curve widget pushes its own undo step when a drag ends, usually
before the timer has written the overrides; the `undo_post` and `redo_post` handlers
complete such a step and never push one. For an edit that Blender records without a step
(a script, or a widget button that pushes none), the timer pushes an *Edit Arbrume Curve*
step once the points stop changing, unless an undo step was pushed since the edit began.
That check reads `WindowManager.undo_stack`; on a Blender without it the timer always
pushes, which can leave a second, empty step after a drag.

Meshes are rebuilt in place: `clear_geometry`, then element counts with `add` and the data
through the generic attribute API (`position`, `.corner_vert`, `material_index`, the UV
map, the colour attribute `Col`, and the generator normals as the `custom_normal`
attribute where Blender reads it as free normals, else
`normals_split_custom_set_from_vertices`). The attribute API copies each buffer in one
call. Bark and leaves are two material slots of one mesh, which keeps a regeneration to
one upload and lets scattered copies share one datablock. Materials are created once per
species, material values and textures: a Principled BSDF whose base colour is the
material colour multiplied by the colour attribute. A textured part adds Image Texture
nodes, the images packed in the file and named after a hash of their bytes so plants of a
species share them: the base colour image multiplies the material colour, its alpha goes
through a *Greater Than* node at the alpha cutoff into the BSDF alpha (an alpha clip that
works in Cycles and EEVEE), the normal image (non-colour) feeds a tangent-space *Normal
Map* node on the UV map, and the roughness image (green channel, non-colour) times the
roughness factor feeds the roughness. The UV map is flipped vertically (`v` becomes
`1 - v`) because Blender puts the origin of images at the bottom, glTF at the top; the
normal maps then need no change, since both conventions point `+y` up the image.

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
platforms; its `blender` job (on a tag, or by hand with *Also build the Blender extension
zips*) packages the Linux x86_64, Windows x64, macOS arm64 and macOS x86_64 wheels into
one zip each with Blender 4.2 LTS (downloaded from download.blender.org and checked
against its published SHA-256), runs `blender --command extension validate` on each zip
and uploads them as the `blender-extension` artifact. The Linux aarch64 wheel is built
but not packaged: the manual's list of extension platforms does not name `linux-arm64`
(Blender 5.3 accepts it).

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
through its node with undo and redo of the edit, an age scrub, every operator, saving
and reopening the file, disabling and enabling the extension, upload timings and two
EEVEE stills. The third opens a window (with `--enable-event-simulate`), draws the
panels, saves screenshots of the sidebar, then drags a point of a curve widget with
simulated mouse events and checks that the drag is one undo step that undo and redo
restore with the plant.

`ARBRUME_TEST_FALLBACKS=1` makes the second and third force the mesh upload paths meant
for Blenders without the faster attribute APIs (corners through `loops.vertex_index`,
normals through `normals_split_custom_set_from_vertices`), so they run on any Blender.
That shows the fallback code works; it does not show that Blender 4.2 takes those paths
or passes the rest of the test. The extension has only been run on Blender 5.3 so far:
Blender 4.2 LTS support is declared (`blender_version_min`) but unverified until the
test runs on a 4.2 build.

## Listing on Superhive

What a paid listing on [Superhive](https://superhivemarket.com) needs from the
maintainer, from Superhive's *General Product Guidelines for Uploading Products* and
*Why Products Get Rejected* (read 2026-11-19; check them again before submitting):

- [ ] **Licence.** Add-ons must be GPL or MIT, and a GPL add-on ships with its sources
  under the GPL. The zip holds the Python sources; the Rust sources of the bundled wheel
  are in the public repository, so link it from the product page and include `LICENSE`
  in the download.
- [ ] **Extension id.** The `id` in `blender_manifest.toml` (`arbrume`) must not belong to
  another creator's product. Check it before submitting.
- [ ] **Extension validity.** Follow Blender's extension guidelines;
  `blender --command extension validate` passes on every zip (the `blender` job runs it).
- [ ] **Images.** At least 5 (the featured image included), PNG or JPG, 2:1 aspect ratio,
  at least 1200 px wide (1200 x 600, 2560 x 1280). Show what the add-on does (sidebar,
  curve editor, age scrub, scatter, LODs), not only renders. No Blender logo or
  "Blender" wordmark as a design element. GIFs are allowed in the gallery, not as the
  featured image.
- [ ] **Description.** In English: what the add-on is, what it does, what sets it apart,
  and what the buyer receives (one zip per platform, the Blender versions tested). No
  placeholder text in any field.
- [ ] **Documentation on the product page.** Installation instructions and the use of
  every feature, written on the page itself; links to videos or external docs alone are
  rejected. An FAQ is recommended, a video optional. *Install* and *Usage* above are a
  starting point.
- [ ] **Support contact.** Creators are asked to answer support messages, aiming for 72
  hours. Name the support channel on the page.
- [ ] **Account.** A payout method (Stripe Connect or PayPal) is required before
  submitting; until a first product is approved, a creator has one product in review and
  its files must pass Superhive's malware scan.
- [ ] **Blender versions.** Only Blender 5.3 has been tested so far (see *Tests*); test
  4.2 LTS before claiming it on the page.

Not found in those pages, so unverified: rules on bundled compiled binaries (the wheel),
on the manifest `website` field, and pricing or revenue share terms.
