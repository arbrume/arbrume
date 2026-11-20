# Command line and generation pipeline

`arbrume` is the batch generator: it grows plants, meshes them and writes one `.glb` per
plant (and per level of detail), with deterministic seeds, resume and a JSONL manifest.
It is a thin layer over the `arbrume-gen` crate, which the Python bindings use as well.

Build it with `cargo build --release -p arbrume`; the binary is
`target/release/arbrume`. `arbrume --help` and `arbrume <command> --help` list every
option.

## Quick start

```sh
arbrume species list
arbrume gen --species broadleaf --count 100 --seed 1 --age-range 10..30 --out out
arbrume gen --species conifer --count 1000 --quality 0.5 --lods 3 --out out --manifest --resume
arbrume gen --species my_oak.toml --count 10 --age 25 --out out
arbrume gen --config batch.toml
arbrume gen --random --count 256 --seed 1 --thumb --out plants
arbrume gen --species open_grown_broadleaf --age 25 --surroundings wall.toml --out out
arbrume gen --species forest_grown_broadleaf --age 40 --growth-frames 20 --out growth
arbrume info my_oak.toml
arbrume bench
```

## Commands

| Command | Does |
|---|---|
| `arbrume gen --species <preset or file.toml> ...` | generate plants of one species |
| `arbrume gen --config batch.toml` | generate the batch described by a config file |
| `arbrume gen --random ...` | generate random plants: a varied individual of a random preset each |
| `arbrume species list` | list the built-in presets (name, display name) |
| `arbrume species show <name>` | print the TOML file of a preset |
| `arbrume info <file.toml>` | validate a species file, then generate one plant with the default parameters and print its statistics |
| `arbrume bench` | time each stage of the pipeline and the batch throughput, print a Markdown table |
| `arbrume metrics <dir>` or `arbrume metrics --presets all` | summarise the ICTree realism features of skeletons per species ([ML annotations](annotations.md#realism-metric-v0)) |
| `arbrume fit --target FILE --from PRESET ...` | fit the parameters of a species to a target skeleton and write a species file ([Parameter fitting](fitting.md)) |
| `arbrume distance A B` | the distance between two tree skeletons, term by term ([Parameter fitting](fitting.md#the-distance)) |
| `arbrume pack new/add-species/validate/thumbs/build/install/list/remove` | author, check, package and install species packs ([Species packs](packs.md)) |
| `arbrume forest --config forest.toml --out DIR` | place a forest over a terrain and write its variants, instanced file and instance tables ([Forests](forest.md)) |

Exit codes: 0 on success, 1 when generating or writing a plant failed (the other plants
are still generated, the failures are listed on stderr), 2 for invalid arguments,
configuration or species files.

## `arbrume gen`

| Option | Default | Meaning |
|---|---|---|
| `--species S` | | preset name, species `pack_id/name` of an installed pack ([Species packs](packs.md)), else path to a species TOML file |
| `--count N` | 1 | number of plants, indexed `0..N` |
| `--seed S` | 0 | base seed of the batch |
| `--age A` | 10 | age in growth cycles |
| `--age-range LO..HI` | | age drawn per plant, uniform over the integers `LO` to `HI` included |
| `--scale X` or `--scale LO..HI` | 1 | scale factor, fixed or drawn uniformly per plant |
| `--quality Q` | 1 (0.6 with `--random`) | mesh quality, as in `MeshOptions::with_quality` |
| `--random` | | draw each plant from a random preset; excludes `--species`, the age and scale options and `--config` |
| `--max-triangles N` | 60000 | with `--random`: triangle budget of a plant, 0 for none |
| `--max-vertices N` | 55000 | with `--random`: vertex budget of a plant, 0 for none |
| `--packs LIST` | `builtin` | with `--random`: draw from the species of these installed packs, comma-separated; `builtin` is the built-in presets |
| `--thumb [SIZE]` | 256 when given | write a thumbnail of `SIZE` by `SIZE` pixels next to each plant |
| `--thumb-format F` | `jpg` | `jpg` or `png` |
| `--lods N` | 1 | mesh levels of detail per plant, each within a triangle budget ([Levels of detail](lod.md)) |
| `--billboard [METHOD]` | `cloud` when given | add a billboard as the last level: `cloud` or `cross` |
| `--wind [LAYOUT]` | `texcoord` when given | bake wind data into every level: `texcoord` or `custom` ([Wind](wind.md)) |
| `--skin` | | add a skeleton with skin weights following the wind data; needs `--wind` |
| `--lod-file` | | write all levels of a plant in one file, one child node per level |
| `--instance-leaves` | | with `--lod-file`: leaves as GPU instances of their templates (`EXT_mesh_gpu_instancing`) |
| `--surroundings FILE` | | TOML file of the envelope, obstacles and neighbours around each plant |
| `--guides FILE` | | JSON file of crown, limb and erase strokes and freezes that steer each plant ([Brush](brush.md)) |
| `--roots HABIT` | `species` | root system of every plant: `tap`, `heart`, `plate` or `fibrous` replace the `[roots]` of the species, `species` keeps it, `none` removes it ([Roots](roots.md)); not applied to `--random` plants |
| `--soil FILE` | | TOML file of the ground surface and resource patches the roots grow in ([Roots](roots.md)) |
| `--growth-frames K` | | write each plant as `K` frames of its growth instead of one file; excludes `--lods` above 1, `--wind`, `--billboard` and `--random` |
| `--out DIR` | | output directory, created if missing |
| `--jobs J` | all cores | worker threads; without it `RAYON_NUM_THREADS` applies |
| `--resume` | | skip plants already complete |
| `--manifest` | | write `manifest.jsonl` in the output directory |
| `--junctions TIER` | `tier-a` | how a branch is joined to its parent: `tier-a` (intersecting tubes with a collar) or `tier-b` (one closed bark surface per stem) ([Junctions](junctions.md)) |
| `--textures MODE` | `files` | procedural bark and leaf textures: `files` (PNG files shared next to the plants), `embed` (inside each file) or `none` ([Textures](textures.md)) |
| `--annotations LIST` | | ML ground truth next to each plant: `skeleton`, `labels`, `points=N`, `branches[=K]`, `metrics`, `json` ([ML annotations](annotations.md)) |
| `--quiet`, `-q` | | print errors only |
| `--config FILE` | | batch config file; excludes the species and parameter options |

**Seeds.** Plant `i` of a batch with base seed `s` gets the seed `plant_seed(s, i)`,
which also seeds its mesh, and its age and scale are drawn from `(s, i)` too (see
`arbrume_gen::Sampler`). The output is therefore the same whatever `--jobs`, whatever
`RAYON_NUM_THREADS`, and whether a plant is generated in a full run or by `--resume`;
the test suite checks that the files are byte-identical.

**Files.** Plant `i` is written to `<species>_<seed>_<i>.glb`, where `<species>` is the
preset name or the file stem of the species file (or the `name` of a config entry) and
`<seed>` the base seed. With `--lods N`, `N > 1`, or `--billboard`, level `k` is
`<species>_<seed>_<i>_lod<k>.glb`: the `N` mesh levels of
`arbrume_gen::Generator::lod_chain`, meshed from the same graph, each within a triangle
budget `0.3^k` times that of level 0, then the billboard
([Levels of detail](lod.md)). With `--lod-file` they are all in
`<species>_<seed>_<i>.glb`, one child node per level. The metadata of each level adds
`lod_kind` (`mesh` or `billboard`), `lod_budget`, `lod_triangles`, `max_screen_height`
(the plant height on screen, in pixels, up to which the level is meant) and `quality`;
the manifest lists each level with its triangles (with `--lod-file`, every level names
the one file). With `--wind` every level carries the [wind data](wind.md). Each worker thread writes its own files and reuses its buffers, so memory stays at
one plant per thread whatever the count.

**Textures.** With `--textures files` (the default) or `embed`, every plant carries
the textures of its species, synthesized once per species by a cache shared by the
worker threads, and its leaves are meshed as cards cut by the alpha of the texture.
`files` writes each image once to the output directory as `arbrume_<hash>.png`, named
after its bytes, and the `.glb` files reference it by relative URI; `embed` stores it in
every file. A random plant takes the textures of its preset. Unless `--quiet`, the run
ends with the number of species textured, the synthesis time and the size of the
images. [Textures](textures.md) gives the file sizes and times measured on the random set.

**Surroundings.** `--surroundings FILE` reads an envelope and occluders, in meters
around a plant standing at the origin ([Growth](growth.md#surroundings) lists the shapes):

```toml
envelope = { shape = "ellipsoid", center = [0.0, 5.0, 0.0], radii = [2.5, 5.0, 2.5] }

[[occluders]]
shape = "box"
min = [1.5, 0.0, -8.0]
max = [2.0, 9.0, 8.0]
```

Every plant of the run grows in them. Species in the self-organizing mode respond;
parametric species are not changed, except climbers. The blocking occluders are also the
supports of climbing species ([Climbing plants](climbing.md)); there is no separate
support option: a support is an occluder, with `shade = 0` when it should not darken the
plant. `examples/climbing/` has a wall with a window, a pergola and rocks:

```sh
arbrume gen --species ivy_like --age 16 --surroundings examples/climbing/window_wall.toml --out ivy
```
 The surroundings are written to the metadata of each
file (`surroundings`), so the plant can be regenerated; a voxel mask, which only the
library can pass, is not written out.

**Guides.** `--guides FILE` reads painted strokes and freezes from a JSON file
([Brush](brush.md)), in meters around a plant standing at the origin, and steers every
plant of the run with them; a parametric species is not changed. The guides are written
to the metadata of each file (`guides`). The Blender extension stores the strokes of a
plant in the same form (`Object.arbrume.guides`), so a file saved from there can be given
here:

```sh
arbrume gen --species open_grown_broadleaf --age 20 --guides strokes.json --out out
```

**Roots.** A species with a `[roots]` table grows a root system below the ground, and
`--roots` gives every plant of the run a habit or removes the roots. `--soil FILE` reads
the ground and the resource patches ([Roots](roots.md#the-soil)); the obstacles of the
roots are the blocking occluders of `--surroundings`, so a rock is an occluder below the
ground:

```sh
arbrume gen --species broadleaf --roots plate --soil soil.toml --surroundings rock.toml --out plate
```

The soil is not written to the metadata of the file.

**Growth sequences.** With `--growth-frames K` a plant is grown once and written as it
stood at `K` ages spread evenly over its growth, the last being the full-grown plant:
`<species>_<seed>_<i>_frame000.glb` to `_frame<K-1>.glb` (fewer when the plant has fewer
cycles than `K`). Each frame holds the metamers born by its cycle, the branches the
plant had not yet shed, the radii its radius model gives at that age and the leaves it
retained; axes keep the position they have in the grown plant
([Growth](growth.md#age-and-growth-sequences)). The manifest lists the frames with their
`cycle`, the metadata of each file records it as `mesh.max_cycle`, the thumbnail shows
the last frame, and `--resume` requires every frame.

**Random plants.** With `--random`, plant `i` is drawn by
`arbrume_gen::RandomSampler` from the base seed and `i` alone: a preset, with a
probability proportional to its `variation.weight`; an age and a scale within its
`[variation]` ranges; an individual of the species, whose parameters stray from the
preset within its `[variation.jitter]` table ([Species library](species.md)); and the
plant seed. The file is named `<preset>_<seed>_<i>.glb`. A plant is meshed at
`--quality` unless that exceeds the triangle or the vertex budget, in which case the
quality is lowered until the mesh fits: a herb keeps its detail and an old tree stays
light. The defaults (quality 0.6, 60 000 triangles, 55 000 vertices) keep every file
under about 3 MB; they are starting points, not measured optima. As everywhere, the
output does not depend on `--jobs`.

**Thumbnails.** `--thumb` renders each plant with the CPU rasterizer
([Rasterizer](raster.md)) from 20 degrees of azimuth, with the default view
and lighting of `ThumbnailOptions`, and writes `<species>_<seed>_<i>.jpg` (JPEG quality
88) or `.png` next to the `.glb`. With several levels of detail the thumbnail shows
level 0. The manifest line of the plant gets a `thumbnail` key, and `--resume` also
requires the thumbnail to exist.

**Metadata.** The root node `extras` and `asset.extras` of every file hold the JSON of
`arbrume_gen::metadata`: generator and version, species display name, plant seed, age,
scale, mesh options, statistics and the full species parameter set, plus `label`,
`base_seed`, `index`, `lod` and `quality`. Deserializing `species` gives back the
species, so every file can be regenerated from its own metadata with the same version.
For a random plant `species` is the individual, with its jittered parameters, `mesh`
holds the options reached under the budget, and `preset` names the preset it was drawn
from.

**Resume.** A file is written under `<name>.tmp` and renamed when complete, so a file
with its final name is whole. With `--resume` a plant is skipped when all its levels
of detail exist and, with `--manifest`, when the manifest has its line; a last manifest
line cut short by an interrupted run is removed first. Other plants are generated again
and their lines appended. Resuming with other parameters but the same seed and names
does not regenerate the existing files.

**Manifest.** `manifest.jsonl` has one JSON object per plant, in completion order. A run
without `--resume` rewrites it, so give each batch its own output directory or resume:

```json
{"species": "broadleaf", "species_name": "Generic broadleaf", "index": 4, "seed": 3,
 "plant_seed": 12963831759638888921, "age": 9, "scale": 1.0, "quality": 1.0,
 "files": [{"file": "broadleaf_3_4.glb", "lod": 0, "quality": 1.0,
            "vertices": 5836, "triangles": 6458, "bytes": 292124}],
 "stats": {"nodes": 452, "chains": 72, "leaf_sites": 255, "vertices": 5836,
           "triangles": 6458, "bounds": {"min": [-1.83, 0.0, -1.9], "max": [1.78, 4.5, 2.37]},
           "height": 4.5},
 "timings_ms": {"grow": 0.6, "mesh": 0.4, "export": 5.3}}
```

`stats` describe level 0; `timings_ms` are wall-clock milliseconds summed over the
levels. With `--annotations` a line also has `annotations` (the sidecar file names),
`species_params` (the full parameter set the plant was grown with), `metrics` when asked
and `annotate_ms` ([ML annotations](annotations.md#manifest)); `--resume` then also
requires the annotation files. Plant seeds are full 64-bit integers: read them with a JSON parser that keeps
integers exact (Python does; JavaScript `JSON.parse` does not).

## Batch config files

```toml
out = "dataset"                 # output directory, relative to this file; --out wins
seed = 42                       # base seed of the entries that set none (default 0)
quality = 1.0                   # mesh quality of the entries that set none (default 1)
lods = 1                        # levels of detail per plant (default 1)
manifest = true                 # same as --manifest (default false)
thumb = 256                     # same as --thumb 256 (default: no thumbnail)
thumb_format = "jpg"            # or "png"
age = { uniform = [10, 30] }    # age of the entries that set none (default 10)
scale = 1.0                     # scale of the entries that set none (default 1)
growth_frames = 0               # same as --growth-frames (default 0: one file per plant)
annotations = "skeleton,labels" # same as --annotations (default: none)
textures = "files"              # same as --textures (default: files)
junctions = "tier-b"            # same as --junctions (default: tier-a)
lod = { wind = "texcoord", billboard = "cloud", file = false, skin = false, instance_leaves = false }

[surroundings]                  # around the plants of the entries that set none
occluders = [{ shape = "box", min = [1.5, 0.0, -8.0], max = [2.0, 9.0, 8.0] }]

[[species]]
preset = "broadleaf"
count = 100

[[species]]
preset = "hedge_shrub"
count = 20
surroundings = { envelope = { shape = "box", min = [-0.5, 0.0, -1.5], max = [0.5, 1.4, 1.5] } }

[[species]]
preset = "broadleaf"
name = "broadleaf_small_leaves" # label in file names; must be unique in the batch
count = 100
overrides = { "foliage.length" = 0.05 }

[[species]]
path = "oak.toml"               # relative to this file
count = 50
seed = 7
quality = 0.5
age = { normal = [25, 5], min = 5, max = 40 }
scale = { normal = [1.0, 0.1], min = 0.7 }

[species.overrides]
"growth.axis_length" = { by_order = [18.0, 5.0, 1.5, 0.4] }
geometry = { pipe_exponent = 2.5 }
```

A distribution is a number (fixed), `{ uniform = [lo, hi] }` or
`{ normal = [mean, std_dev], min = .., max = .. }`, a normal draw being clamped to `min`
and `max` where given. Ages are rounded to whole cycles, and a uniform age is uniform
over the integers from `lo` to `hi` included. A distribution that could draw an age
outside `[0, 65535]` or a scale of zero or less is rejected: give a normal `min` (and
`max` for an age).

`overrides` change parameters of the species file before it is parsed. A key is a
dotted path to a parameter (`"growth.branching_angle"`) and its value replaces the whole
parameter: a number, a curve, an array, or a table such as `architecture.phyllotaxis`
(whose missing fields then take their defaults). A section key with a table value
(`geometry = { pipe_exponent = 2.5 }`, or the TOML dotted key
`geometry.pipe_exponent = 2.5`) sets those parameters of the section and keeps the
others. Elements of arrays such as `[[architecture.axes]]` cannot be addressed; replace
the whole array. The result is validated like any species file, so a misspelt path is an
error.

### Stands

A config file may also list the plants of one stand: plants of any species, each at
its place, grown together so that they shade one another (see
[Several plants](growth.md#several-plants-stands)). `docs/stand.toml` is a stand of three,
and `examples/stands/` holds a grove of seven trees and a hedgerow of seven plants.

```toml
quality = 0.5                   # mesh quality of every plant of the stand
stand_file = true               # also write stand.glb, like --stand-file (default false)

[[stand]]
species = "open_grown_broadleaf" # preset, pack species `pack_id/name`, or species file
position = [-3.0, 0.0, 0.0]     # meters, Y up
age = 14                        # growth cycles
seed = 1                        # seed of this plant, used as is
scale = 1.0                     # optional (default 1)
name = "big_tree"               # optional label in file names (default the species)

[[stand]]
species = "excurrent_conifer"
position = [3.0, 0.0, 0.0]
age = 16
seed = 2
```

```sh
arbrume gen --config docs/stand.toml --out stand --stand-file --thumb 512
```

Plant `i` is written as `stand_<i>_<label>_<seed>.glb`, in its own frame with its base
at the origin; its node `extras` hold the usual metadata plus `stand_index` and
`position`. `stand_file = true` or `--stand-file` writes the whole stand as `stand.glb`
too: a root node (`extras`: generator, version, number of plants) with one child node
per plant, translated to its position, each with its own mesh and `extras`. Textures
shared by several plants are stored once. With `--thumb`, `stand.<format>` shows the
whole stand.

The top-level `quality`, `textures`, `junctions` and `[surroundings]` apply to the
stand; the surroundings are in the frame of the stand, not of a plant. A stand is
written at one level of detail: `lods > 1`, growth frames, wind, billboards and
annotations are rejected (exit code 2). A config file may hold `[[species]]` entries
too; they are generated as a batch first, the stand after. The plants of a stand do not
depend on the order of the `[[stand]]` entries nor on `--jobs`; the file names do, since
they carry the index.

## `arbrume forest`

```sh
arbrume forest --config examples/forests/valley.toml --out out/valley
```

Places the forest of a config file over its terrain, grows the variant library of each
species, and writes one `.glb` per variant, one instanced `.glb`
(`EXT_mesh_gpu_instancing`), the instance tables (CSV, binary) and a JSON manifest
([Forests](forest.md) documents the config file and every output).

| Flag | Default | Effect |
|---|---|---|
| `--config FILE` | required | the forest config (`forest.toml`) |
| `--out DIR` | required | output directory, created if missing |
| `--seed N` | the config's | replace the seed |
| `--placement-only` | off | grow nothing: write the tables and the manifest only (every species needs a `spacing`) |

It prints the trees of each species against the target on open ground, warns when a
density is out of reach for its spacing, and lists the files with their sizes. Exit code
2 for an invalid config, terrain or exclusion, 1 when growing or writing failed.

## `arbrume bench`

For every species (`--species a,b`, default all presets) and age (`--ages`, default
8, 20, 40, those of the `arbrume-grow` benchmark), one plant (base seed 1, index 0) is
generated `--repeats` times (default 5) with a warm generator, and the median time of
growth, meshing and export to memory is printed with the node and triangle counts and
the file size. The throughput columns time a batch of `--batch` plants (default 32;
0 skips them) generated and exported to memory, on one thread and on every thread of
the pool. Nothing is written to disk, so the disk is not measured: a batch of large
plants on many cores can be limited by the write speed of the disk instead.

`--annotations LIST` adds the annotations of `LIST` to every plant, computed and
serialized to memory, their time counted in the export column and in the throughput
([ML annotations](annotations.md#throughput) has the comparison).

`cargo bench -p arbrume-gen` measures the same stages with criterion
(`pipeline/<stage>/<preset>/<age>`).

The self-organizing presets, measured this way on one 16-thread desktop (a record of one
machine, not a target; `grow`, `mesh` and `export` are medians in milliseconds):

| species | age | nodes | triangles | grow | mesh | export | file MB | trees/s (1 thread) | trees/s (16 threads) |
|---|---|---|---|---|---|---|---|---|---|
| `open_grown_broadleaf` | 10 | 226 | 16 669 | 0.29 | 0.20 | 0.12 | 1.09 | 2448 | 3937 |
| `open_grown_broadleaf` | 20 | 3 958 | 193 071 | 3.21 | 1.81 | 0.97 | 12.81 | 152 | 376 |
| `open_grown_broadleaf` | 30 | 13 805 | 426 636 | 10.89 | 4.06 | 2.27 | 28.89 | 53 | 175 |
| `open_grown_broadleaf` | 40 | 25 422 | 315 296 | 20.05 | 3.78 | 1.62 | 21.12 | 39 | 182 |
| `forest_grown_broadleaf` | 10 | 79 | 5 718 | 0.05 | 0.06 | 0.06 | 0.39 | 6222 | 10089 |
| `forest_grown_broadleaf` | 20 | 1 981 | 99 367 | 1.02 | 0.87 | 0.36 | 6.56 | 464 | 1090 |
| `forest_grown_broadleaf` | 30 | 7 060 | 190 178 | 4.84 | 1.75 | 0.93 | 12.36 | 134 | 468 |
| `forest_grown_broadleaf` | 40 | 13 449 | 213 078 | 10.60 | 2.06 | 1.05 | 13.86 | 64 | 341 |
| `excurrent_conifer` | 10 | 265 | 10 351 | 0.16 | 0.14 | 0.08 | 1.03 | 2074 | 5215 |
| `excurrent_conifer` | 20 | 1 846 | 66 027 | 1.96 | 1.12 | 0.57 | 6.63 | 237 | 849 |
| `excurrent_conifer` | 30 | 5 253 | 164 282 | 6.22 | 2.88 | 1.49 | 16.50 | 90 | 341 |
| `excurrent_conifer` | 40 | 10 167 | 243 272 | 12.11 | 4.39 | 2.30 | 24.52 | 53 | 213 |
| `spreading_decurrent` | 10 | 237 | 18 714 | 0.21 | 0.40 | 0.18 | 1.26 | 1801 | 7701 |
| `spreading_decurrent` | 20 | 2 826 | 117 670 | 2.20 | 1.41 | 0.76 | 8.05 | 232 | 781 |
| `spreading_decurrent` | 30 | 6 890 | 158 176 | 6.90 | 1.97 | 0.96 | 10.52 | 99 | 498 |
| `spreading_decurrent` | 40 | 9 867 | 105 813 | 10.83 | 1.45 | 0.71 | 7.04 | 76 | 491 |
| `thicket_shrub` | 10 | 688 | 28 263 | 0.41 | 0.32 | 0.17 | 1.50 | 1303 | 2932 |
| `thicket_shrub` | 20 | 2 390 | 36 488 | 1.67 | 0.42 | 0.22 | 1.88 | 443 | 2245 |
| `hedge_shrub` | 10 | 946 | 23 668 | 0.40 | 0.36 | 0.14 | 2.38 | 630 | 3030 |
| `hedge_shrub` | 20 | 3 604 | 89 195 | 2.58 | 1.98 | 0.85 | 9.32 | 192 | 810 |
| `envelope_tree` | 10 | 116 | 7 986 | 1.80 | 0.12 | 0.08 | 0.57 | 549 | 4980 |
| `envelope_tree` | 20 | 1 905 | 129 090 | 5.57 | 1.11 | 0.69 | 9.42 | 71 | 505 |
| `envelope_tree` | 30 | 1 943 | 131 936 | 7.23 | 1.57 | 0.85 | 9.64 | 48 | 417 |

Measured on an AMD Ryzen 7 9700F (8 cores, 16 logical processors) under Windows 11,
with no other build or benchmark running.

`arbrume bench --engine` prints another table for the species in the self-organizing
mode: the growth time of one plant with the default engine options, with a sparse shadow
grid, with the buds sensed on the pool and with the markers given to buds on one thread,
then the throughput of plants grown one per task with the default options and with
nothing parallel inside a plant. [Growth](growth.md#performance-and-engine-options) has
the measurements and the defaults taken from them; `cargo bench -p arbrume-gen --bench
engine` measures the same options and the two data structures alone with criterion.

`arbrume bench --junctions` prints the bark triangles and the meshing time of each species
and age with Tier A and with Tier B junctions ([Junctions](junctions.md#cost) has the
measurements); `cargo bench -p arbrume-mesh` compares them on synthetic skeletons
(group `junctions`).

## Library

```rust
use arbrume_gen::{GenParams, Generator, Plant, generate, generate_many, preset};

let species = preset("broadleaf").unwrap();
let plant = generate(&species, &GenParams::new(7, 20).with_quality(0.5))?;
println!("{} triangles, {:.1} m", plant.stats.triangles, plant.stats.height);
```

| Item | Use |
|---|---|
| `generate(species, params)` | one plant: graph, mesh, statistics |
| `GenParams { seed, age, scale, mesh, surroundings }`, `GenParams::new(seed, age)`, `.with_scale(x)`, `.with_quality(q)`, `.with_surroundings(s)` | parameters of one plant |
| `Surroundings { envelope, occluders }`, `Shape`, `Occluder`, `VoxelMask` | what stands around a plant, re-exported from `arbrume-grow` |
| `Generator::timeline(species, params)`, `Timeline::frame_cycles(k)`, `Generator::frame(species, &timeline, cycle, &mut plant)` | growth sequences: one growth, meshed at several ages |
| `Plant { params, graph, mesh, stats }` | the result; `graph` is the skeleton for sidecar exporters |
| `Stats { nodes, chains, leaf_sites, vertices, triangles, bounds, height }` | summary; `height` is the vertical extent of the mesh |
| `Generator::new()`, `.generate(..)`, `.generate_into(.., &mut plant)`, `.remesh(&mut plant, species, options)` | batch loops; reuses the mesher and the mesh buffers, returns timings |
| `Generator::generate_within(species, params, &budget, &mut plant)`, `Budget { triangles, vertices }` | the same within a mesh budget: the quality is lowered until the mesh fits, and `plant.params.mesh` records the options used |
| `RandomSampler::new(seed)`, `.pick(index)`, `RandomPick { preset, species, params, budget }`, `.generate(&mut generator, &mut plant)` | random plants: a preset, an age, a scale and a varied individual per index |
| `generate_many(&[(species, params)])` | a batch on the current `rayon` pool, results in input order |
| `Generator::generate_group(&[(species, params, position)])`, `GroupEntry` | a stand: the plants grown together so that they shade one another, then meshed; one plant per entry, in input order, each in its own frame |
| `run_batch(count, \|generator, plant, index\| ..)` | a batch that consumes each plant in the task, one generator and one scratch plant per thread |
| `Sampler { seed, age, scale, quality }`, `.params(index)`, `plant_seed(seed, index)` | the per-plant parameters of a dataset |
| `Distribution` | fixed, uniform or normal parameter distribution (TOML deserializable) |
| `presets()`, `preset(name)`, `preset_toml(name)` | built-in species |
| `species_with_overrides(toml, overrides)` | a species file with fields changed by dotted path |
| `metadata(species, plant)` | the JSON written to the `extras` of a file |

Every result depends only on the species, the parameters and the version: a batch gives
the same plants as `generate` on each job, for any number of threads.

**Presets** live in `crates/arbrume-gen/species/` and are described in
[Species library](species.md). To add one, drop `<name>.toml` there and add its line to
`PRESETS` in `crates/arbrume-gen/src/presets.rs`; the tests then generate and export it,
and CI validates its files.

## Looking at species

`arbrume sheet` is a development command, hidden from `--help`, that renders a contact
sheet of thumbnails into one PNG, for tuning a species by eye:

```sh
arbrume sheet --species oak_like,pine_like --ages 15,30,60 --seeds 2 --azimuths 20,110 --out sheet.png
arbrume sheet --species my_oak.toml --ages 40 --view crown --size 512 --out crown.png
arbrume sheet --random 64 --offset 64 --seeds 1 --size 160 --out random_1.png
arbrume sheet --species open_grown_broadleaf --ages 12,22 --surroundings wall.toml --out wall.png
arbrume sheet --species forest_grown_broadleaf --ages 40 --growth-frames 6 --out frames.png
arbrume sheet --species hedge_shrub --ages 6,12 --row 5 --spacing 0.8 --surroundings box.toml --out hedge.png
arbrume sheet --stand examples/stands/grove.toml --azimuths 20,110 --size 640 --out grove.png
```

A species sheet has one row per species, seed and azimuth and one column per age; a
random sheet shows plants `offset` to `offset + N` of the random sampler with base seed
`--seeds`. `--view` is `full`, `crown` (the middle of the crown, enlarged) or `junction`
(the trunk and its first branches); `--junctions` chooses the junctions as for `gen`. One
line per plant goes to standard output: quality
reached, height, width, trunk diameter, triangles, vertices, nodes, leaf sites and time.

`--surroundings FILE` grows every plant in the surroundings of the file and draws its
blocking occluders. `--growth-frames K` replaces the age columns by `K` frames of the
growth of the oldest age, all framed like the last. `--row N` grows `N` plants of the
species side by side along X, `--spacing` meters apart (default 4), with
`arbrume_grow::grow_together`, so that they share their light; with surroundings the
row is centred on their origin. `--stand FILE` shows the `[[stand]]` of a config file
instead: per azimuth, the stand (left) and the same plants grown alone, placed at the
same positions and framed alike (right). One line per plant compares its height, crown
width and node count in the stand and alone, and how far the centre of its nodes moved.
