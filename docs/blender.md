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
  species and kept by the extension for the session, or the image textures of a pack
  species.
- **Species packs.** The species menu lists the built-in presets, then the species of
  each installed pack ([Species packs](packs.md)) under a heading with the pack name and
  version, each with its thumbnail as icon. *Edit > Preferences > Add-ons > Arbrume*,
  box *Species Packs*, shows the pack directory (shared with the command line and the
  Python module), installs a `.arbpack` file (*Install Pack*, which validates it first),
  lists the installed packs with their version and content licence, removes one (trash
  button) and reads the directory again (*Refresh Packs*). A plant stores the id of its
  pack species; if the pack is removed, the plant keeps its mesh and shows the error at
  its next regeneration.
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
- **Surroundings.** For a species in the self-organizing mode
  ([Growth](growth.md#surroundings)), the *Surroundings* sub-panel names an *Envelope*
  object the crown grows inside and a list of *Obstacles* (*+* adds the selected mesh
  objects, or an empty slot to pick one in), each blocking growth, casting shade of a
  given strength, or both. They can be any mesh object; their modifiers are applied and
  they are read in world space, relative to the plant. On regeneration each is voxelized
  into the frame of the plant: the inside of a closed mesh plus every voxel its surface
  crosses, so an open plane still works as a wall. *Voxel Size* 0 (the default) takes
  half the shortest internode of the species times the plant scale, printed under the
  field; the grid of one object is capped at 8 million voxels, the voxels growing to
  fit. A trunk rises into an envelope that floats above the ground as it does into an
  analytic one: outside a voxel mask the generator sees the distance to its nearest
  filled voxel. *Trunk Passage* (off by default) also opens a column of voxels from the
  ground up to the envelope under the plant, for a crown that should keep a bare stem
  inside its volume. A voxelized object is cached by a hash of its evaluated geometry, its
  transform relative to the plant and the voxel settings, so dragging a slider of the
  plant reuses it; the panel shows the voxel count, the time spent and how many objects
  came from the cache. Moving an envelope or an obstacle does not regenerate the plant
  by itself: press *Regenerate* or change a setting. For a parametric species the panel
  says the volumes have no effect, and nothing is voxelized.
- **Supports.** Each obstacle slot has a *Role*: *Obstacle* (blocks and casts the shade set
  beside it) or *Support*, a surface a climbing plant holds, which blocks growth and casts
  no shade. A climbing species (`ivy_like`, `vine_like`, `creeper_like`, or any species
  with a `[climbing]` table, see [Climbing plants](climbing.md)) climbs every blocking
  slot, in either growth mode; the support role only saves it from the shade of the
  object it climbs. Supports are voxelized and cached like obstacles. The automatic voxel
  size follows the internodes of the species; for a thin post or a wire set *Voxel Size*
  below the radius of the support, or the plant sees a coarser object. A plant mesh can
  be a support too, for ivy on a tree: select it as a support slot.
- **Brush.** The *Brush* sub-panel and the *Arbrume Brush* tool of the toolbar paint
  strokes that steer a self-organizing plant (see [Brush](#brush) below).
- **Water.** The *Water* sub-panel gives an aquatic species ([Underwater plants](underwater.md))
  its water: *Water Surface* is *Species* (still water as deep as the species says),
  *Height* (the surface at a height above the plant) or *Object* (the surface at the top
  of an object, such as a plane: its world bounding box less the height of the plant);
  *Current* is a steady current along X and Y in m/s. A land species shows that it
  ignores the water. Moving the surface object does not regenerate the plant by itself.
- **Roots.** The *Roots* sub-panel chooses the root system of the plant ([Roots](roots.md)):
  *Species* grows the `[roots]` of the species when it has one (the presets
  `*_rooted`), *Grow Roots* adds the heart habit to a species without roots, *No Roots*
  grows none. Below it, the parameters of `[roots]` from the schema (habit, mode,
  primaries, lengths and angles per order, sinkers, pipe balance, flare, exposure) are
  edited like any other; changing one adds the layer to a species without it. Roots grow
  below the ground plane of the plant (negative Z). The *Obstacles* of the Surroundings
  panel block roots too, for a parametric species as well: a sphere under the ground is a
  stone the roots grow around.
- **Growth Frames.** *Batch and Files > Growth Frames* grows the active plant once and
  makes one object per growth stage (*Stages*, spread evenly over its age, with its
  surroundings) in a new collection `<plant> growth`. Their viewport and render
  visibility are keyframed so that one stage shows at a time, each for *Frames per
  Stage* frames from the scene start, the last staying to the end; the scene is made
  long enough. Scrub the timeline to watch the plant grow. The plant itself is hidden
  (*Hide Plant*). The stages are separate meshes because the number of vertices changes
  from one stage to the next, which shape keys cannot hold; each stage object records
  its last growth cycle in the custom property `arbrume_growth_cycle`. The stages are
  plain meshes: editing the plant does not change them.
- **Batch and Files.** *Duplicate with New Seed*; *Scatter Variations* makes N plants in a
  grid next to the active one or at the other selected objects, with a number of distinct
  seeds: plants with the same seed share one mesh. *Create LODs* makes copies at
  decreasing quality in a new collection, named `_LOD0`, `_LOD1`, ...
  *Save Species File* writes the species of the plant with its changes as a standalone
  TOML file; *Load Species File* uses such a file for the active plant or a new one.

A plant is one mesh object. What it is made from is stored on the object (preset name or
species file text, seed, age, scale, quality, the changed parameters as JSON, and the
envelope and obstacle objects), so a
saved file reopens editable and a plant can be duplicated or relinked like any object.
Editing the mesh by hand is overwritten at the next regeneration.

Foliage is baked into the mesh by the generator; leaves as instances are not available
yet.

Preferences (*Edit > Preferences > Add-ons > Arbrume*): the quality of new plants, the
number of threads of batch generation, and whether to use the generator's normals.

## Forests

*Add > Mesh > Arbrume Forest* (`bpy.ops.arbrume.add_forest`) places a forest over a
terrain mesh, the active object by default. Its redo panel takes:

| Property | Default | Meaning |
|---|---|---|
| *Terrain* | active mesh | the mesh the trees stand on |
| *Species Mix* | `excurrent_conifer:90,forest_grown_broadleaf:50` | species (preset or `pack_id/name`) and trees per hectare |
| *Density* | 1 | factor on every density |
| *Seed* | 0 | seed of the placement and of the variants |
| *Variants*, *Mesh Levels*, *Billboard*, *Textures* | 3, 2, cloud, on | the variant library ([Forests](forest.md#variant-library)) |
| *Terrain Resolution* | 2 m | spacing of the height samples |
| *Viewport Level* | last | level of detail drawn in the viewport; renders draw level 0 |
| *Config* | none | a `forest.toml` whose terrain is replaced by the mesh: exclusions, slopes, altitudes, modes |
| *Water* | none | *Height* (*Water Height*) or *Object* (*Water Object*, its top): the water level; aquatic species of the mix stand under it, the others above ([Forests](forest.md#aquatic-species)) |

The terrain is sampled by casting rays straight down onto its evaluated mesh on a grid of
the chosen resolution; samples off the mesh take the heights of their neighbours and are
excluded (a `mask_grid` exclusion), so nothing grows off the mesh. The forest is computed by `arbrume.forest`, then made of:

- a collection `<terrain> forest library`, excluded from the view layer, with one
  collection per level of detail holding one mesh object per variant;
- one object `<terrain> forest` whose vertices are the trees, with the point attributes
  `arbrume_index` (which variant), `arbrume_rotation` (quaternion), `arbrume_scale` and
  `arbrume_tint`, and a Geometry Nodes modifier: *Instance on Points* picks the variant
  among the separate children of the collection of the viewport level (*Is Viewport*) or
  of level 0 in renders, and turns and scales it.

The materials of the forest are copies of the plant materials (`<material> tinted`) whose
base colour is multiplied by `arbrume_tint`, read from the instancer. Whatever the number
of trees the scene holds a few objects: the valley of [Forests](forest.md) (17 339 trees,
3 species, 11 variants, 4 levels) is 44 variant objects and one instancer, built in
0.6 s from the result of `arbrume.forest`. The trees are instances, not objects: to edit
one, *Make Instances Real* on the instancer.

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
| `packs` | the species of installed packs as groups of the species menu |
| `voxels` | meshes voxelized in the frame of a plant (parity fill of the inside, surface shell, trunk passage), the voxel size from the species, the cache key of an object |
| `frames` | the visibility keys of growth frames |
| `water` | the water of a plant (surface from a height or an object, current turned from Z up to Y up) and the water level of a forest |

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

## Brush

The *Arbrume Brush* tool (toolbar of the 3D view in Object Mode, or *Brush Tool* in the
*Brush* sub-panel) paints strokes on the active plant ([Brush](brush.md) has the growth
rules). Each left-button drag is one stroke and one undo step:

- **Crown** paints a volume the crown grows into. *Radius* is the brush radius,
  *Strength* the light given to the buds that reach it, *Reach* the distance from which
  buds are drawn to it, *Density* its markers.
- **Limb** draws a line one axis follows: start it on the trunk or on a branch (the
  closest bud within the radius of the start takes it), or at the ground to steer the
  trunk. *Follow* is how closely the axis keeps to the line.
- **Erase** removes growth and keeps it out. With *Erase Through* (the default) the
  stroke cuts along the view through the whole plant, as an eraser on a picture does;
  without it only near the drawing plane.

A stroke is projected onto a plane facing the view through the point of the plant (or of
any other object) under its first point, or through the plant origin when there is none
(*Projection: Plane*), or onto the surfaces under the mouse (*Surface*). While the button
is held, the plant grows again with the stroke so far at *Preview Quality* each time the
mouse has moved, as often as generation keeps up; on release it grows at its own
quality. *Esc* or the right button cancels the stroke. The strokes are drawn over the
view while the tool is active (*Show Strokes* draws them always): crown green, limb
orange, erase red.

*Freeze* keeps the plant grown so far: the age grows by *Growth After Freeze* cycles, in
which only later strokes add growth; *Unfreeze* removes the last freeze and the strokes
drawn after it, *Clear Strokes* every stroke. The panel counts the strokes, shows the
time of the last stroke and warns about a limb no branch took or one that is not grown
to its end (raise the age). The strokes are stored on the object (`Object.arbrume.guides`,
the JSON of [Brush](brush.md)) in the frame of the plant, so they move, turn and
duplicate with it, and a saved file reopens editable.

For a parametric species the panel says that strokes steer self-organizing species and
offers to switch to the self-organizing preset closest in habit: trunk height, branch
reach, number of stems, highest order, and branches in tiers or not
(`logic.brush.nearest_self_organizing`). The switch drops the parameter changes of the
plant, which belong to the other species.

Time of one stroke drawn in twelve steps (`brush_steps.time_strokes`, Blender 5.3 in
background on the machine of [Growth](growth.md#performance-and-engine-options)):

| plant | triangles at quality 1 | each preview at quality 0.25 (median, max) | on release: total (growth and meshing, upload) |
|---|---|---|---|
| `envelope_tree`, age 16 | 90 239 | 14.7 ms, 16.9 ms | 36.0 ms (9.6 ms, 25.7 ms) |
| `open_grown_broadleaf`, age 20 | 208 913 | 19.7 ms, 22.2 ms | 73.5 ms (11.6 ms, 60.3 ms) |
| `open_grown_broadleaf`, age 30 | 449 380 | 46.5 ms, 49.0 ms | 161.0 ms (29.6 ms, 129.0 ms) |

The upload of the mesh into Blender, not growth, dominates on release. These were taken
with no other build running; with several compilations running on the machine, the same
strokes took two to four times as long.

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
and reopening the file, a species pack installed from an archive made by Python's
`zipfile` (its heading, species and thumbnail in the species menu, the Packs box of the
preferences drawn through a stand-in layout, a plant of its species with the pack images
on its materials, then its removal), a plant grown in a sphere envelope (the crown stays
inside it, a seed change reuses the cached voxels, moving the sphere voxelizes it
again), growth frames of that plant (as many objects as stages, one visible at each
frame of the timeline), a plant next to a cube obstacle (no skeleton node inside the
cube, where the same plant without it grows through) and the same obstacle ignored by a
parametric species (`blender/tests/surroundings_steps.py`), an ivy given a wall in the
support role (it creeps without it; with it, it climbs above 1.5 m, no skeleton node
enters the wall, most nodes hold it, the occluder casts no shade and a seed change
reuses the voxels; `blender/tests/climbing_steps.py`, whose `render` also makes the
Cycles still of an ivy on a wall with a window), a forest over a hilly grid (`blender/tests/forest_steps.py`: the
library excluded from the view layer with one object per variant and level, tinted
materials, every tree on the terrain, one instance per tree, the same trees for the same
seed and others for another, a negative density refused, an EEVEE still), a rooted
preset with its roots turned off and on, a root parameter, and a stone under the ground
that no root node enters (`blender/tests/roots_steps.py`, whose `render_still` makes a
Cycles still of a rooted spruce next to a stone, the ground left out), a bull kelp under
the surface of its species, of a height and of a plane, bent along X by a current, a shrub
that ignores the water, and a forest over a sloping seabed with kelp only under a water
plane and herbs only above it (`blender/tests/water_steps.py`), disabling and
enabling the extension, upload timings and three EEVEE stills, the last a plant in a sphere envelope
next to a wall. The third opens a window (with `--enable-event-simulate`), draws the
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
