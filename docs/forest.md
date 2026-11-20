# Forests

`arbrume-forest` spreads thousands to millions of trees over a landscape and keeps them
cheap to store and to draw: a few variants of each species are grown and meshed once, as
a chain of [levels of detail](lod.md), and every tree is one record that references a
variant with its own transform. This page describes the placement, the variant library,
the files, coupled growth in a region, what branch-module instancing would add, the
command line, Python and Blender entry points, and what it all costs.

```sh
arbrume forest --config examples/forests/valley.toml --out out/valley
```

```python
import arbrume

placed = arbrume.forest("examples/forests/valley.toml")
placed.positions  # (n, 3) float32, meters, Y up
placed.library[0][2]["levels"][0]  # level 0 of variant 2 of the first species, an arbrume.Tree
```

| Piece | Entry point |
|---|---|
| terrain from a 16-bit PNG, a grid of heights or procedural hills | `Terrain` |
| clearings, roads, rivers, polygons, painted masks, water level | `Exclusions` |
| species mix with densities, spacing, slope and altitude preferences | `SpeciesRule` |
| placement | `place(&terrain, &exclusions, &rules, &PlaceSettings)` |
| variant library | `build_library(&[VariantSpec], &LibrarySettings)` |
| coupled region | `grow_coupled(&instances, &specs, &CoupledSettings)` |
| the whole config | `ForestConfig`, `Forest::build`, `write_forest` |
| instanced `.glb` | `arbrume_export::write_glb_forest` |
| module sharing measurement | `arbrume_forest::modules::module_report` |

## Placement

### Candidates and thinning

The ground is cut into square cells as wide as the largest conflict distance between two
trees, so that two trees in conflict always lie in the same or in neighbouring cells.
Each cell draws Poisson candidates for each species from its own random stream, keyed by
the seed, the cell and the species. A candidate is kept with the probability of its own
suitability (below) and of the exclusion factor at its place; then it survives when no
candidate of higher priority stands closer than the conflict distance of the pair.

This is the second hard-core process of Matérn (1960), with marks for the species: a
candidate is removed by any candidate of higher priority within reach, even one that is
itself removed. Every decision then reads only a candidate's neighbourhood, so the
thinning is one parallel pass, and the density it leaves has a closed form, which is what
makes the densities controllable:

- a candidate of species `s` with priority rank `u` is beaten by a Poisson number of
  candidates of mean `(1 - u) L_s`, with `L_s = sum_t lambda_t pi d_st^2`;
- it survives with probability `(1 - exp(-L_s)) / L_s`, averaged over `u`;
- the candidate intensities `lambda` that leave the target densities `rho` solve
  `lambda_s (1 - exp(-L_s)) / L_s = rho_s`, by fixed-point iteration from `lambda = rho`
  (`arbrume_forest::density`).

The thinned process cannot be denser than `1 / (pi d^2)` for one species of spacing `d`
however many candidates are drawn (about 199 trees per hectare at 4 m, 354 at 3 m, 1 273
at 1.5 m). A target out of reach is reported, per species, as a count of *saturated*
cells: the intensity is then capped where `L_s` reaches 30, and the density reached is
close to the ceiling. Lower the density or the spacing (or raise `overlap`).

Matérn's third process (thinning in priority order, a candidate beaten only by kept
ones) packs about twice as densely, but a decision there can depend on candidates far
away through chains of conflicts, so it is neither one parallel pass nor bounded to a
band; it is not implemented.

**Modes.**

- `poisson` (default): random priorities; the conflict distance of two species is the
  mean of their spacings. The densities are the targets, up to sampling (the tests check
  it within four standard deviations of a Poisson count, a loose bound since hard-core
  counts vary less than Poisson counts).
- `competition`: the larger tree wins (priority by crown size, `spacing` times the
  relative scale of the tree), and the conflict distance is the mean of the two scaled
  spacings: large trees suppress the small ones near them, the self-thinning by distance
  and size of ecosystem simulations (Makowski et al. 2019 grow it; here it is one pass
  over candidates). The intensities are those of the Poisson mode at the mean scales, so
  the density is close to the target but not equal: on the benchmark terrain below, 1.5 %
  fewer trees than the Poisson mode; on `examples/forests/valley.toml`, 2.7 % under the
  open-ground target for the conifers (part of the gap is the soft edges of the
  exclusions, which the target leaves out).

**Layers.** Species of different `layer` values do not compete for space: shrubs of
layer 1 grow under trees of layer 0. Their densities are independent.

**Suitability.** A species takes slopes in `slope = [low, high]` (degrees) and ground
heights in `altitude = [low, high]` (meters) fully, and fades linearly to nothing over
`slope_falloff` and `altitude_falloff` outside them. A cell draws at the largest
suitability over its centre and corners; each candidate is then kept with its own
suitability over that of the cell, so a limit holds tree by tree.

**Exclusions.** Each exclusion gives a factor in `[0, 1]`: 0 inside its shape, rising
linearly to 1 over `falloff` meters outside (a soft forest edge). Factors multiply. A
candidate removed by an exclusion never competes, so trees along a clearing or a road
are as dense as inside the forest, not denser.

**Bands and memory.** The ground is processed in bands of cell rows (about 2^18 cells
each); a band also draws the rows next to it, and a ring of cells outside the terrain,
as competitors only. The result therefore depends neither on the band size nor on the
number of threads (tested with 1, 3, 4 and 8 threads and bands of 1, 7 and automatic
rows), and the memory of the candidates is bounded by a band. Trees come back in band,
row, cell and candidate order.

**Each tree** gets, from a stream keyed by its cell and its slot in the cell: a variant
among the species' `variants`, a yaw, a lean of up to `lean` degrees about a random
horizontal axis, a scale uniform in `scale`, and a colour factor `1 + tint * u` per tree
times `1 + tint / 3 * v` per channel (`u`, `v` uniform in `[-1, 1)`). Its base is the
lowest ground under its trunk (centre and four points on the rim of the trunk radius of
its variants, times its scale), lowered by `sink`.

### Terrain

A `Terrain` is a regular grid of heights over a rectangle, bilinear in between, clamped
at the edges; slopes come from the gradient of that surface. Image rows run along `+z`
from the top, columns along `+x` from the left. Paths of exclusions with `carve` lower
the terrain along their centre line (a river bed, easing out over the band and its
falloff), before the placement reads it.

## Variant library

Variant `k` of a species is `species.varied(seed, k)` (its `[variation.jitter]`) grown
with `plant_seed(seed, k)` at the species' age, then meshed as a level-of-detail chain:
`lods` mesh levels and a billboard ([Levels of detail](lod.md)), with the textures of
the species shared by all its variants ([Textures](textures.md)). Variants are built in
parallel, one generator each. The default spacing of a species is
`2 (1 - overlap)` times the mean crown radius of its variants (half their mean
horizontal extent) times its mean scale.

## Files

`write_forest` writes into the output directory:

| File | Content |
|---|---|
| `<species>_v<k>.glb` | variant `k`, one child node per level `<species>_v<k>_LOD<i>` (billboard last), with `lod`, `lod_kind`, `lod_triangles` and `max_screen_height` in the `extras` |
| `<name>_instanced.glb` | one group node per level, `<name>_LOD<i>`, holding one node per variant that instances its level-`i` mesh at every tree of that variant (`EXT_mesh_gpu_instancing`) |
| `<name>_instances.csv` | `species,variant,x,y,z,qx,qy,qz,qw,scale,r,g,b` per tree |
| `<name>_instances.bin` | 32-byte header, then one 40-byte record per tree (below) |
| `<name>_instances.json` | the CSV rows as JSON arrays, for small forests |
| `<name>_coupled.glb` | the trees of the coupled region, each a node at its place |
| `<name>.json` | manifest: species, rules, placement counts, variants with their files, sizes and levels, files and timings |

**Instanced file.** Each instancing node carries `TRANSLATION`, `ROTATION` (unit
quaternion), `SCALE` and `_COLOR_0`, the colour factor of each tree as linear RGB
floats. The extension allows application-specific attributes whose names start with an
underscore; three.js (`GLTFLoader`, checked with r167) turns `_COLOR_0` into the
`instanceColor` of the `InstancedMesh`, which its materials multiply into the base
colour. The Khronos validator reports the files without errors or warnings (it lists
the instance accessors as possibly unused, as infos, since it does not read the
extension). Every level of every variant is instanced at every tree, so a reader that
does not pick levels by distance draws them all on top of one another: hide the groups
it does not want, or write only some with `output.levels`. Blender expands the
extension into one object per instance ([Wind](wind.md#gltf-carriage-trial)); use the
Blender operator below instead. Coupled trees are left out of it.

**Binary table.** Little-endian. Header: magic `ARBINST1`, version (`u32`, 1), record
size (`u32`, 40), record count (`u64`), 8 zero bytes. Record: position (3 `f32`),
rotation (4 `f32`, `x y z w`), scale (`f32`), species (`u16`), variant (`u16`, 65535 for a
coupled tree, which is node `coupled_<index>` of the coupled file), colour factor (4
`u8`, `value / 128`). In NumPy it is `arbrume.INSTANCE_DTYPE` read from byte 32.

## Coupled growth

`[coupled]` grows the trees of a rectangle together, so that neighbours shade one another
([Several plants](growth.md#several-plants-stands)), instead of drawing them from the
library. The rectangle is cut into square tiles; each tile grows, as one stand, the
trees whose base lies in it plus those within `halo` meters of it, which shade the tile
and are then dropped. Memory is that of one tile's stand per thread whatever the size of
the region, and tiles run in parallel. A tree is grown upright at its place and scale as
its variant individual with the seed of its variant; its yaw and lean are not applied,
since the shade depends on them. Coupling is exact inside a tile and approximate across
tile borders (two trees on either side of a border are not in the same stand). Only
species in the self-organizing mode respond to shade.

Measured with `excurrent_conifer` (age 25, 150 per hectare) and `forest_grown_broadleaf`
(age 30, 60 per hectare), tiles of 40 m, halo 10 m, mesh quality 0.5, no textures:

| Region | Coupled trees | Tiles | Largest stand | Time | Coupled file | Peak memory |
|---|---|---|---|---|---|---|
| 40 x 40 m | 36 | 1 | 76 | 0.65 s | 184 MB | 0.77 GB |
| 80 x 80 m | 136 | 4 | 84 | 1.06 s | 694 MB | 2.0 GB |
| 120 x 120 m | 309 | 9 | 83 | 2.23 s | 1.63 GB | 3.8 GB |

Growth is not the limit: each coupled tree is a unique mesh of a few megabytes, about
5 MB in the file and 12 MB of memory at quality 0.5, so a region of a few hundred trees
is the practical scale, and the coupled file reaches the 4 GiB limit of a `.glb` near
800 trees at that quality.

## Branch modules

Makowski et al. (2019) build every plant of an ecosystem from a small set of branch
modules, stored once and drawn many times. Arbrume grows each variant as one free graph,
so `modules::module_report` asks how far the branches of a library could be shared
after the fact: every lateral axis of one order, with what it bears, is a module;
modules are compared by the chamfer distance of their node positions in a frame at
their base, normalized by the length of their axis, and reduced greedily to prototypes
(same number of axes, error at most a tolerance times the module length). It reports the
vertices of level 0 before and after replacing every module by its prototype, plus a
40-byte transform per module.

`cargo run --release -p arbrume-forest --example modules -- broadleaf:20
excurrent_conifer:25 pine_like:30`, four variants each:

| Plants | Order | Tolerance | Modules | Prototypes | Vertices kept | Mean / max error |
|---|---|---|---|---|---|---|
| broadleaf | 1 | 5 % | 92 | 87 | 100 % | 0.02 / 0.02 m |
| broadleaf | 2 | 10 % | 415 | 127 | 49 % | 0.07 / 0.16 m |
| excurrent_conifer | 1 | 10 % | 468 | 86 | 44 % | 0.11 / 0.42 m |
| excurrent_conifer | 2 | 5 % | 1 744 | 314 | 55 % | 0.02 / 0.09 m |
| pine_like | 2 | 5 % | 3 019 | 110 | 20 % | 0.05 / 0.16 m |
| all twelve | 2 | 5 % | 5 178 | 668 | 42 % | 0.04 / 0.15 m |
| all twelve | 2 | 10 % | 5 178 | 178 | 27 % | 0.07 / 0.30 m |

So sharing sub-branches (order 2) at a 5 % shape error would keep 20 % to 55 % of the
vertices of a library; main branches (order 1) only share once errors reach tens of
centimeters. Two things make this a measurement rather than an exporter: a shared module
would need its junction re-stitched to its bearing axis (collars and watertight
junctions are built per plant, [Junctions](junctions.md)), and the saving applies to
the library, which a forest stores once: for the valley below the variant files and the
instanced file are about 265 MB for 11 variants, the table of its 17 339 trees 0.7 MB. Module instancing pays
where geometry is unique per tree, as in a coupled region, which is not built from
modules either. It is not implemented as an export.

## Config (`forest.toml`)

Paths are relative to the config file. Every table but `[terrain]` and `[[species]]` is
optional; unknown keys are errors.

```toml
name = "valley"          # file names (default "forest")
seed = 7                 # placement and variants

[terrain]
size = [600.0, 600.0]    # meters along x and z
origin = [-300.0, -300.0]  # smallest x and z (default: centred on the origin)
height = 0.0             # flat ground, when nothing else is given
# heightmap = "terrain.png"  # grayscale PNG, 8 or 16 bits
# heights = [0.0, 100.0]     # meters of black and white
# grid = { columns = 3, rows = 2, heights = [...] }  # row-major, rows along z
# hills = { amplitude = 110.0, wavelength = 320.0, octaves = 4, resolution = 2.0, seed = 5 }

[placement]
mode = "competition"     # "poisson" (default) or "competition"
# water_level = 12.0     # no land species where the ground is lower, aquatic species only there
# band_rows = 0          # cell rows per band, 0 automatic; the result does not depend on it

[[species]]
species = "excurrent_conifer"  # preset, pack species `pack_id/name`, species file or TOML text
# name = "pine"          # label in files (default: the preset, pack id or file stem)
density = 150.0          # trees per hectare on ground that suits the species
# spacing = 3.0          # smallest trunk distance at the mean scale, meters
overlap = 0.65           # for the default spacing: 2 (1 - overlap) x mean crown radius x mean scale
layer = 0                # species of other layers do not compete with it
age = 25                 # of the variants (default: the oldest age of [variation])
variants = 4
scale = [0.6, 1.25]      # default: [variation].scale of the species
slope = [0.0, 35.0]      # degrees taken fully
slope_falloff = 10.0
altitude = [60.0, 200.0] # ground heights taken fully, meters
altitude_falloff = 10.0
lean = 3.0               # largest lean, degrees
tint = 0.08              # half-width of the colour factor
sink = 0.0               # extra depth of the base, meters
# depth = [6.0, 15.0]   # aquatic species: water depths taken fully, below placement.water_level
# depth_falloff = 1.0
# fit_depth = true       # scale each aquatic tree to the water over its base

[[exclude]]              # exactly one of circle, path, polygon, mask, mask_grid
path = { points = [[-300.0, -60.0], [0.0, 25.0], [300.0, 40.0]], width = 10.0 }
falloff = 6.0            # soft edge, meters
carve = 3.0              # paths only: lower the terrain along the centre line
# circle = { center = [x, z], radius = r }
# polygon = { points = [[x, z], ...] }
# mask = "mask.png"      # grayscale over the terrain, white excluded, grey thins
# mask_grid = { columns = 4, rows = 2, values = [...] }  # the same as values, cells row-major

[library]
lods = 3                 # mesh levels
billboard = "cloud"      # "cloud", "cross" or "none"
atlas = 512              # billboard atlas side, texels
textures = true
wind = false             # wind data in the meshes (texcoord layout)
quality = 1.0            # mesh quality of level 0

[output]
variants = true          # one .glb per variant
instanced = true         # the instanced .glb
# levels = [1, 3]        # levels in the instanced file (default all)
tables = ["csv", "bin"]  # also "json"

[coupled]                # optional
region = [-20.0, -20.0, 20.0, 20.0]  # x0, z0, x1, z1
tile = 40.0
halo = 10.0
quality = 0.5
```

The defaults of the species rule (`overlap` 0.5, 4 variants, the slope and altitude
windows wide open) and of the library are starting values chosen for the look of the
example renders, not measurements.

## Aquatic species

A species with an `[aquatic]` table ([Underwater plants](underwater.md)) stands under
`placement.water_level`, where land species do not: its rule is `underwater`, so the
exclusions free the ground below the water level for it and close the ground above it.
A config with an aquatic species needs a water level. Two keys of `[[species]]` are read
for them only:

| Key | Default | Meaning |
|---|---|---|
| `depth` | any | `[shallow, deep]`, water depths taken fully, in meters below the water level: the altitude window `[level - deep, level - shallow]` |
| `depth_falloff` | 0 | fade outside `depth`, in meters |
| `fit_depth` | true | scale each tree to the water over its base (below) |

The variants are grown in still water as deep as the `aquatic.water_depth` of the
species, the depth its parameters were tuned for. With `fit_depth` every tree is then
scaled by the depth of the water over its base divided by that depth (kept within
`underwater::FIT_RANGE`, 0.25 to 4), so its canopy, floats and pads reach the surface
wherever it stands; its scale range then only sets its spacing. `examples/underwater/`
has a kelp forest on a shelving rocky seabed and a pond with lilies, tape grass,
waterweed and herbs on the bank.

```sh
arbrume forest --config examples/underwater/kelp_forest.toml --out out/kelp_forest
```

## Command line, Python, Blender

`arbrume forest --config FILE --out DIR [--seed N] [--placement-only]` builds and
writes a forest and prints the trees per species against the open-ground target, the
saturated cells, the timings and the files ([Command line](cli.md)). With
`--placement-only` nothing is grown and only the tables and manifest are written; every
species then needs a `spacing`.

`arbrume.forest(config, *, grow=True, base=None, out=None, threads=None)` takes a dict
with the keys above, TOML text or a path, and returns an `arbrume.Forest`
([Python bindings](python.md#forests)).

In Blender, *Add > Mesh > Arbrume Forest* samples a terrain mesh and instances the
forest on it with Geometry Nodes ([Blender extension](blender.md#forests)).

## Performance

Measured on one 16-thread Windows desktop (AMD Ryzen 7 9700F), release build, best of
three runs; a record of one machine, not a target.

**Placement** (`cargo run --release -p arbrume-forest --example place_bench -- 1000 3162
10000`): square terrains of procedural hills, two canopy species (150 and 60 per hectare,
spacings 3 and 4 m, slope and altitude preferences) and an understory layer (600 per
hectare, spacing 1.5 m), a carved river and ten clearings:

| Side | Mode | Trees | Time | Trees per second | Result |
|---|---|---|---|---|---|
| 1 km | poisson | 76 458 | 8 ms | 9.9 M | 3.1 MB |
| 3.16 km | poisson | 796 790 | 79 ms | 10.1 M | 54 MB |
| 10 km | poisson | 8 043 379 | 0.74 s | 10.9 M | 432 MB |
| 10 km | competition | 7 921 798 | 0.71 s | 11.1 M | 613 MB |

A tree is 40 bytes; the result column also counts the spare capacity of the vector. A
band held at most 0.7 million candidates.

**Whole forests** (`arbrume forest` on `examples/forests/valley.toml` with the terrain
side changed; 11 variants, 3 mesh levels and a billboard each, textured; peak working set
of the process):

| Trees | Side | Peak memory | Library | Instanced `.glb` | `.bin` table | CSV | All files |
|---|---|---|---|---|---|---|---|
| 981 | 145 m | 394 MB | 0.69 s | 129.9 MB | 39 kB | 101 kB | 266 MB |
| 9 884 | 460 m | 413 MB | 0.68 s | 131.7 MB | 395 kB | 1.0 MB | 269 MB |
| 101 046 | 1 450 m | 400 MB | 0.65 s | 150.7 MB | 4.0 MB | 10.3 MB | 301 MB |

The library dominates memory and files; a tree adds 40 bytes to the table and 208 bytes
to the instanced file (52 bytes per level). Expanding the 101 046 trees as meshes at
level 0 (50 000 to 130 000 triangles each) would be some ten billion triangles.

In Blender the valley (17 339 trees) is 47 objects: 44 variant meshes, the instancer,
the ground and the river; building it from the result of `arbrume.forest` takes 0.6 s.

## Limits

- The variants of aquatic species grow in still water: a current set for a forest has no
  carriage yet, and a tree fitted to a much shallower or deeper place is a scaled copy,
  not a plant grown there.

- Hard-core densities are capped at `1 / (pi d^2)` per species; dense natural stands
  need a smaller spacing than the crowns suggest (or `overlap` near 1), and the
  competition mode reaches the targets only approximately.
- Suitability is drawn per cell at its most suitable point and thinned per tree; the
  intensity is not recomputed per tree, so densities in a fade are approximate.
- Placement is not a growth simulation: no succession, no seed dispersal, no climate
  (Palubicki et al. 2022 couple growth to soil water and clouds); a coupled region grows
  trees together but does not kill or seed any.
- The instanced file holds every level for every tree; picking levels by distance is
  left to the engine (Bruneton and Neyret 2012 switch from instances to a shaded
  coverage map in the distance, which is not attempted here).
- Billboards are static clouds or crossed quads ([Levels of detail](lod.md)); the lobe
  textures of Livny et al. (2011) for distant crowns are not implemented.
- Coupled trees are unique meshes: a few hundred at most.
- Deussen-style ecosystem simulations were not available in the reference library and
  were not consulted.

## References

- Bruneton, E. and Neyret, F. 2012. Real-time realistic rendering and lighting of
  forests. Computer Graphics Forum 31(2).
- Décoret, X., Durand, F., Sillion, F. and Dorsey, J. 2003. Billboard clouds for extreme
  model simplification. ACM Transactions on Graphics 22(3).
- Fuhrmann, A., Umlauf, E. and Mantler, S. 2005. Extreme model simplification for forest
  rendering. Eurographics Workshop on Natural Phenomena.
- Livny, Y., Pirk, S., Cheng, Z., Yan, F., Deussen, O., Cohen-Or, D. and Chen, B. 2011.
  Texture-lobes for tree modelling. ACM Transactions on Graphics 30(4).
- Makowski, M., Hädrich, T., Scheffczyk, J., Michels, D. L., Pirk, S. and Pałubicki, W.
  2019. Synthetic silviculture: multi-scale modeling of plant ecosystems. ACM
  Transactions on Graphics 38(4).
- Matérn, B. 1960. Spatial variation. Meddelanden från Statens Skogsforskningsinstitut
  49(5).
- Pałubicki, W., Makowski, M., Gajda, W., Hädrich, T., Michels, D. L. and Pirk, S. 2022.
  Ecoclimates: climate-response modeling of vegetation. ACM Transactions on Graphics
  41(4).
