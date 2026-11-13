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

Exit codes: 0 on success, 1 when generating or writing a plant failed (the other plants
are still generated, the failures are listed on stderr), 2 for invalid arguments,
configuration or species files.

## `arbrume gen`

| Option | Default | Meaning |
|---|---|---|
| `--species S` | | preset name, else path to a species TOML file |
| `--count N` | 1 | number of plants, indexed `0..N` |
| `--seed S` | 0 | base seed of the batch |
| `--age A` | 10 | age in growth cycles |
| `--age-range LO..HI` | | age drawn per plant, uniform over the integers `LO` to `HI` included |
| `--scale X` or `--scale LO..HI` | 1 | scale factor, fixed or drawn uniformly per plant |
| `--quality Q` | 1 (0.6 with `--random`) | mesh quality, as in `MeshOptions::with_quality` |
| `--random` | | draw each plant from a random preset; excludes `--species`, the age and scale options and `--config` |
| `--max-triangles N` | 60000 | with `--random`: triangle budget of a plant, 0 for none |
| `--max-vertices N` | 55000 | with `--random`: vertex budget of a plant, 0 for none |
| `--thumb [SIZE]` | 256 when given | write a thumbnail of `SIZE` by `SIZE` pixels next to each plant |
| `--thumb-format F` | `jpg` | `jpg` or `png` |
| `--lods N` | 1 | levels of detail per plant |
| `--surroundings FILE` | | TOML file of the envelope, obstacles and neighbours around each plant |
| `--growth-frames K` | | write each plant as `K` frames of its growth instead of one file; excludes `--lods` above 1 and `--random` |
| `--out DIR` | | output directory, created if missing |
| `--jobs J` | all cores | worker threads; without it `RAYON_NUM_THREADS` applies |
| `--resume` | | skip plants already complete |
| `--manifest` | | write `manifest.jsonl` in the output directory |
| `--quiet`, `-q` | | print errors only |
| `--config FILE` | | batch config file; excludes the species and parameter options |

**Seeds.** Plant `i` of a batch with base seed `s` gets the seed `plant_seed(s, i)`,
which also seeds its mesh, and its age and scale are drawn from `(s, i)` too (see
`arbrume_gen::Sampler`). The output is therefore the same whatever `--jobs`, whatever
`RAYON_NUM_THREADS`, and whether a plant is generated in a full run or by `--resume`;
the test suite checks that the files are byte-identical.

**Files.** Plant `i` is written to `<species>_<seed>_<i>.glb`, where `<species>` is the
preset name or the file stem of the species file (or the `name` of a config entry) and
`<seed>` the base seed. With `--lods N`, `N > 1`, level `k` is
`<species>_<seed>_<i>_lod<k>.glb`, meshed from the same graph at quality
`Q * 0.5^k`: every level has roughly half the sides, rings and leaves of the previous
one. Each worker thread writes its own files and reuses its buffers, so memory stays at
one plant per thread whatever the count.

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
parametric species are not changed. The surroundings are written to the metadata of each
file (`surroundings`), so the plant can be regenerated; a voxel mask, which only the
library can pass, is not written out.

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
levels. Plant seeds are full 64-bit integers: read them with a JSON parser that keeps
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

## `arbrume bench`

For every species (`--species a,b`, default all presets) and age (`--ages`, default
8, 20, 40, those of the `arbrume-grow` benchmark), one plant (base seed 1, index 0) is
generated `--repeats` times (default 5) with a warm generator, and the median time of
growth, meshing and export to memory is printed with the node and triangle counts and
the file size. The throughput columns time a batch of `--batch` plants (default 32;
0 skips them) generated and exported to memory, on one thread and on every thread of
the pool. Nothing is written to disk, so the disk is not measured: a batch of large
plants on many cores can be limited by the write speed of the disk instead.

`cargo bench -p arbrume-gen` measures the same stages with criterion
(`pipeline/<stage>/<preset>/<age>`).

The self-organizing presets, measured this way on one 16-thread desktop (a record of one
machine, not a target; `grow`, `mesh` and `export` are medians in milliseconds):

| species | age | nodes | triangles | grow | mesh | export | file MB | trees/s (1 thread) | trees/s (16 threads) |
|---|---|---|---|---|---|---|---|---|---|
| `open_grown_broadleaf` | 10 | 231 | 17 195 | 0.23 | 0.16 | 0.14 | 1.13 | 2488 | 3154 |
| `open_grown_broadleaf` | 20 | 4 331 | 218 970 | 3.49 | 1.29 | 1.16 | 14.59 | 142 | 322 |
| `open_grown_broadleaf` | 30 | 16 954 | 590 920 | 15.28 | 3.73 | 3.22 | 40.39 | 41 | 114 |
| `open_grown_broadleaf` | 40 | 32 499 | 513 149 | 27.54 | 3.27 | 2.99 | 34.57 | 28 | 98 |
| `forest_grown_broadleaf` | 10 | 329 | 23 241 | 0.17 | 0.15 | 0.15 | 1.51 | 3408 | 5307 |
| `forest_grown_broadleaf` | 20 | 2 835 | 94 486 | 1.85 | 0.77 | 0.50 | 6.23 | 310 | 679 |
| `forest_grown_broadleaf` | 30 | 8 222 | 210 141 | 7.23 | 1.82 | 1.22 | 13.66 | 109 | 402 |
| `forest_grown_broadleaf` | 40 | 15 408 | 200 598 | 15.04 | 1.84 | 1.65 | 12.94 | 55 | 288 |
| `excurrent_conifer` | 10 | 265 | 10 535 | 0.27 | 0.17 | 0.15 | 1.08 | 1659 | 4347 |
| `excurrent_conifer` | 20 | 2 066 | 80 250 | 2.69 | 0.94 | 0.73 | 8.41 | 233 | 668 |
| `excurrent_conifer` | 30 | 6 428 | 222 295 | 5.38 | 2.06 | 1.98 | 23.23 | 96 | 257 |
| `excurrent_conifer` | 40 | 12 435 | 326 323 | 10.79 | 3.37 | 3.13 | 34.02 | 54 | 165 |
| `spreading_decurrent` | 10 | 237 | 15 256 | 0.19 | 0.16 | 0.16 | 0.97 | 3041 | 4283 |
| `spreading_decurrent` | 20 | 2 826 | 95 440 | 1.61 | 0.54 | 0.43 | 6.09 | 353 | 904 |
| `spreading_decurrent` | 30 | 6 890 | 129 524 | 4.84 | 0.75 | 0.64 | 8.00 | 147 | 511 |
| `spreading_decurrent` | 40 | 9 867 | 88 143 | 7.90 | 0.56 | 0.46 | 5.49 | 103 | 469 |
| `thicket_shrub` | 10 | 688 | 17 415 | 0.31 | 0.18 | 0.14 | 1.04 | 1786 | 8460 |
| `thicket_shrub` | 20 | 2 390 | 23 408 | 1.21 | 0.17 | 0.10 | 1.33 | 608 | 3108 |
| `hedge_shrub` | 10 | 946 | 23 668 | 0.43 | 0.28 | 0.14 | 2.38 | 975 | 2047 |
| `hedge_shrub` | 20 | 3 604 | 89 195 | 1.76 | 0.98 | 0.62 | 9.32 | 268 | 709 |
| `envelope_tree` | 10 | 116 | 7 986 | 1.09 | 0.08 | 0.08 | 0.57 | 419 | 1745 |
| `envelope_tree` | 20 | 1 905 | 129 090 | 5.24 | 0.86 | 0.87 | 9.42 | 64 | 407 |
| `envelope_tree` | 30 | 1 943 | 131 936 | 7.38 | 0.91 | 0.97 | 9.63 | 50 | 325 |

`arbrume bench --engine` prints another table for the species in the self-organizing
mode: the growth time of one plant with the default engine options, with a sparse shadow
grid, with the buds sensed on the pool and with the markers given to buds on one thread,
then the throughput of plants grown one per task with the default options and with
nothing parallel inside a plant. [Growth](growth.md#performance-and-engine-options) has
the measurements and the defaults taken from them; `cargo bench -p arbrume-gen --bench
engine` measures the same options and the two data structures alone with criterion.

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
```

A species sheet has one row per species, seed and azimuth and one column per age; a
random sheet shows plants `offset` to `offset + N` of the random sampler with base seed
`--seeds`. `--view` is `full`, `crown` (the middle of the crown, enlarged) or `junction`
(the trunk and its first branches). One line per plant goes to standard output: quality
reached, height, width, trunk diameter, triangles, vertices, nodes, leaf sites and time.

`--surroundings FILE` grows every plant in the surroundings of the file and draws its
blocking occluders. `--growth-frames K` replaces the age columns by `K` frames of the
growth of the oldest age, all framed like the last. `--row N` grows `N` plants of the
species side by side along X, `--spacing` meters apart (default 4), with
`arbrume_grow::grow_together`, so that they share their light; with surroundings the
row is centred on their origin.
