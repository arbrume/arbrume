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

[[species]]
preset = "broadleaf"
count = 100

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
| `GenParams { seed, age, scale, mesh }`, `GenParams::new(seed, age)`, `.with_scale(x)`, `.with_quality(q)` | parameters of one plant |
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
```

A species sheet has one row per species, seed and azimuth and one column per age; a
random sheet shows plants `offset` to `offset + N` of the random sampler with base seed
`--seeds`. `--view` is `full`, `crown` (the middle of the crown, enlarged) or `junction`
(the trunk and its first branches). One line per plant goes to standard output: quality
reached, height, width, trunk diameter, triangles, vertices, nodes, leaf sites and time.
