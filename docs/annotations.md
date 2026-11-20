# ML annotations

Every plant Arbrume generates comes with exact ground truth: the generator knows which
axis or leaf each vertex belongs to, where every metamer is and when it grew. `arbrume gen
--annotations LIST` writes that ground truth next to each `.glb`:

```sh
arbrume gen --species broadleaf --count 1000 --age-range 10..30 --out data --manifest \
    --annotations skeleton,labels,points=4096,branches,metrics
```

| Item of `LIST` | Writes |
|---|---|
| `skeleton` | nodes, chains and organ sites in `<stem>.ann.npz` |
| `labels` | per-vertex semantic class and instance id in `<stem>.ann.npz`, and as the `_SEMANTIC` and `_INSTANCE` vertex attributes of every `.glb` of the plant |
| `branches[=K]` | the branch table in `<stem>.ann.npz`, `K` points per polyline (default 8, at least 2) |
| `points=N` | `N` labelled surface samples in `<stem>.points.ply` |
| `metrics` | the ICTree features of the skeleton in the manifest line ([Realism metric](#realism-metric-v0)) |
| `json` | a JSON copy of the sidecar, `<stem>.ann.json` |

`<stem>` is the file name of the plant without `.glb` (`broadleaf_1_42`). A batch config
takes the same list as `annotations = "skeleton,labels"`. With `--lods` the sidecars
describe level 0 and every level carries its own `_SEMANTIC` and `_INSTANCE`; with
`--growth-frames` every frame carries them and the sidecars describe the last frame.
`--resume` requires the annotation files as well as the `.glb`.

Everything is a pure function of the plant: the files are byte-identical whatever
`--jobs`, as the test suite checks.

## Classes and instances

| Value | Class | What | Instance id |
|---|---|---|---|
| 0 | `trunk` | axes of branching order 0 (each stem of a multi-stemmed plant) | chain id |
| 1 | `branch` | every other axis of the graph | chain id |
| 2 | `twig` | the stem of a twig template (`foliage.twig.stem_radius > 0`) | organ index |
| 3 | `leaf` | leaves, needles, and the fins that stand for needles on a twig | organ index |
| 4 | `root` | axes of the root system, below the collar ([Roots](roots.md)) | chain id |

Classes follow the structure of the plant graph ([Plant graph](core.md)). The bark part
of the mesh holds the axes, the leaf part the instances of the foliage template at the
organ sites; a twig template is a stem followed by its fins and leaves, so the leading
vertices of each instance are `twig` and the rest `leaf`. A species without twig
templates has no `twig` vertex. Classes are structural, not botanical: the rachis of a
palm frond is an axis of the graph and is labelled `branch`, its leaflets `leaf`.

The instance id of a bark vertex is the chain id of its axis, the row of which is found
through `chain_id` in the skeleton; that of a foliage vertex is the index of its organ
site (`organ_*` arrays). Two id spaces, told apart by the class. Every triangle has the
three labels of its vertices equal.

## `.glb` attributes

With `labels`, each primitive gets two application-specific attributes (glTF 2.0,
section 3.7.2.1), one value per vertex:

| Attribute | Accessor | Value |
|---|---|---|
| `_SEMANTIC` | `SCALAR`, `FLOAT` | class value, 0 to 4 |
| `_INSTANCE` | `SCALAR`, `FLOAT` | instance id |

Floats are what glTF allows here: `UNSIGNED_INT` is reserved for indices, and the Khronos
validator rejects 1- and 2-byte scalar vertex attributes because vertex elements must be
4-byte aligned. A `float` holds integers exactly below 2^24 (16 777 216), far above the
chain and organ counts of a plant; the writer refuses larger ids. The files pass the
Khronos validator with no error or warning. The cost is 8 bytes per vertex, about 14 % of
the `.glb` of a leafy tree.

The writer takes such attributes from any caller: `arbrume_export::write_glb_with(mesh,
options, &[ExtraAttribute { part, name, components, data }], out)` with `f32`, `u16` or
`u8` data.

## The sidecar `<stem>.ann.npz`

An uncompressed NumPy archive: `numpy.load` opens it, each array is a plain `.npy`
entry, little-endian, C order. `meta` is a `uint8` array holding the UTF-8 bytes of a
JSON object:

| Key of `meta` | Value |
|---|---|
| `format`, `version` | `"arbrume-annotations"`, 1; the version rises on any incompatible change |
| `semantic_classes` | `["trunk", "branch", "twig", "leaf", "root"]` |
| `organ_kinds`, `part_kinds` | `["leaf", "twig", "flower", "fruit"]`, `["bark", "leaf", "billboard"]` |
| `branch_samples` | `K`, or null without the branch table |
| `arrays` | for each array present: `dtype`, symbolic `shape`, `meaning` |
| `extras` | `stem`, `species_name`, `seed` (the plant seed), `age`, `scale`, `max_cycle` |

Only the groups asked for are present. `N` nodes, `C` chains, `O` organ sites, `P`
primitives, `V` vertices, `B` branches:

| Array | dtype | Shape | Meaning |
|---|---|---|---|
| `node_position` | `<f4` | N, 3 | distal end of each node (metamer), meters, Y up |
| `node_radius` | `<f4` | N | internode radius, meters |
| `node_parent` | `<i4` | N | row of the parent node, -1 for the first node of a root axis |
| `node_order` | `|u1` | N | branching order, 0 for the trunk |
| `node_birth` | `<u2` | N | growth cycle in which the node appeared; its age is `meta.extras.age - birth` |
| `node_chain` | `<u4` | N | chain id |
| `node_shed` | `|u1` | N | 1 when the node was shed |
| `node_dist_root` | `<f4` | N | arc length from the base of the plant to the node, meters |
| `node_dist_chain` | `<f4` | N | arc length from the base of the axis to the node, meters |
| `chain_id` | `<u4` | C | chain id of each chain row |
| `chain_first`, `chain_last` | `<u4` | C | rows of the first and last nodes |
| `chain_parent` | `<i4` | C | chain id of the parent axis, -1 for a root axis |
| `chain_attach` | `<i4` | C | row of the node of the parent the axis grows from, -1 for a root axis |
| `chain_order` | `|u1` | C | branching order |
| `chain_origin` | `<f4` | C, 3 | base point: the attachment node, or the base of a root axis |
| `organ_node` | `<u4` | O | row of the node bearing the organ site |
| `organ_kind` | `|u1` | O | 0 leaf, 1 twig, 2 flower, 3 fruit |
| `organ_direction`, `organ_normal` | `<f4` | O, 3 | unit frame of the organ |
| `organ_scale` | `<f4` | O | size factor on the species organ size |
| `organ_birth` | `<u2` | O | growth cycle in which the organ appeared |
| `part_kind` | `|u1` | P | kind of each `.glb` primitive, in file order: 0 bark, 1 leaf, 2 billboard |
| `part_offsets` | `<u4` | P + 1 | first vertex of each primitive, then `V` |
| `vertex_semantic` | `|u1` | V | class of each vertex, primitives concatenated |
| `vertex_instance` | `<u4` | V | instance id of each vertex |
| `branch_chain` | `<u4` | B | chain id of each branch |
| `branch_parent` | `<i4` | B | row of the parent branch, -1 for a root axis |
| `branch_attach` | `<f4` | B | where the branch leaves its parent: arc length to the attachment node over the live length of the parent, 0 base to 1 tip |
| `branch_order` | `|u1` | B | branching order |
| `branch_birth` | `<u2` | B | growth cycle of its first node |
| `branch_scalars` | `<f4` | B, 4 | live nodes, arc length (m), base radius (m), tip radius (m) |
| `branch_direction` | `<f4` | B, 3 | unit direction of the first internode |
| `branch_polyline` | `<f4` | B, K, 4 | `K` points `(x, y, z, r)` evenly spaced by arc length from origin to tip |

### Skeleton: the node signature, in depth-first order

Each node row is the node signature of Zhou et al. (2023): position, radius, parent,
order and age, with the arc lengths to the root and to the base of the axis that their
signature also carries. Rows are in the depth-first order of
`PlantGraph::depth_first_order`: from a node, the traversal follows its own axis to the
tip, then the axes attached to it, from the tip of the parent down to its base. Hence:

* a parent row always comes before its children (`node_parent[i] < i`);
* the subtree of any node is one contiguous run of rows starting at it;
* each axis is one contiguous run, `chain_first` to `chain_last`, base to tip;
* chain rows are in the order in which the traversal reaches them (`chain_first`
  increasing), so a parent axis comes before its children.

Shed nodes stay in the skeleton with `node_shed = 1`; they are a suffix of their axis and
have only shed descendants. `arbrume_export::annotations::Skeleton::to_graph` rebuilds
the `PlantGraph` from the arrays (same chain ids, same organ sites, without buds), and
the tests check that the rebuilt graph passes `check_invariants` and is equal to the
original.

### Branch table

The branch table is the serialization of Wang et al. (2025), who found depth-first order
the best for autoregressive tree models: one row per axis that has a live node, in the
order of the chain rows (shed axes are left out with their descendants), so a parent row
precedes its children and every subtree is a contiguous run of rows. The polyline of a
branch starts at its origin, on the centerline of its parent, and runs through the
distal ends of its live nodes; `branch_polyline` resamples it at `K` points evenly spaced
by arc length, each with the radius of the internode it lies on (the radius of the first
internode at the origin). Its first and last points are the two endpoints `(x, y, z, r)`
of the representation of Wang et al.; `K = 2` gives exactly that representation. The
attachment position is a fraction of the live length of the parent, so it stays in
`[0, 1]`.

### Labels

`vertex_semantic` and `vertex_instance` are the values of `_SEMANTIC` and `_INSTANCE`
for all primitives of the `.glb` in file order; `part_offsets` cuts them per primitive.
A mesh with parts the mesher did not produce (billboards) is not labelled.

## The JSON form `<stem>.ann.json`

The same content as nested lists, under the same keys, with `meta` as an object.
`tools/read_annotations.py` turns it back into arrays equal to those of the `.npz` (a
test checks this). It is about twice the size of the `.npz` and ten to twenty times
slower to load; use it to look at a plant, not for training.

## Point clouds `<stem>.points.ply`

Scan-to-skeleton methods such as AdTree (Du et al. 2019) and that of Livny et al. (2010)
start from a point cloud of a single tree. `points=N` samples `N` points uniformly by area
over every triangle of level 0, bark and foliage: a triangle is drawn with probability
proportional to its area, then a point uniform on it with the square-root mapping of
Osada et al. (2002). Each point takes the interpolated vertex normal (unit) and the labels
of its triangle. Point `i` depends only on the mesh seed of the plant and `i`, so the
cloud is deterministic and a prefix of it is a smaller cloud with the same distribution.

The file is binary little-endian PLY, 29 bytes per point:

```text
ply
format binary_little_endian 1.0
comment arbrume annotations 1
comment semantic 0 trunk 1 branch 2 twig 3 leaf
element vertex N
property float x
property float y
property float z
property float nx
property float ny
property float nz
property uchar semantic
property uint instance
end_header
```

The samples are of the surface the renderer draws, leaves included: there is no scanner
model (occlusion, noise, range). Leaf points dominate a leafy crown; filter by
`semantic <= 1` for a wood-only cloud.

## Manifest

With any annotation the manifest line of a plant gets:

| Key | Value |
|---|---|
| `annotations` | `{"sidecar": ..., "json": ..., "points": ...}`, the files written |
| `species_params` | the full parameter set of the species the plant was grown with: for a random plant, its jittered individual |
| `metrics` | with `metrics`: `{"local": {feature: {count, mean, var, min, max}}, "global": {feature: value}}` |
| `annotate_ms` | milliseconds spent on the annotations |

The seed is already there (`plant_seed`, with `seed` and `index`). `species_params` is
the nested JSON of the species; `parameter_vector` in `tools/read_annotations.py`
flattens its numbers and booleans into a vector with sorted dotted names.

## Loading with NumPy

`tools/read_annotations.py` needs only the standard library and NumPy:

```python
import sys

sys.path.insert(0, "tools")
import numpy as np
import read_annotations as ra

ann = ra.load_sidecar("data/broadleaf_1_42.ann.npz")
positions, parents = ann["node_position"], ann["node_parent"]
edges = np.stack([parents[parents >= 0], np.flatnonzero(parents >= 0)], axis=1)
wood = ann["vertex_semantic"] <= 1  # trunk and branch vertices
rows = ra.chain_rows(ann)  # chain id -> chain row
order_of_vertex = ann["chain_order"][[rows[i] for i in ann["vertex_instance"][wood]]]

points = ra.load_points("data/broadleaf_1_42.points.ply")
xyz, labels = points["xyz"], points["semantic"]

line = ra.read_manifest("data")[0]
names, vector = ra.parameter_vector(line["species_params"])
```

Without the helper, `np.load(path)` gives the arrays and
`json.loads(np.load(path)["meta"].tobytes())` the metadata.
`python tools/read_annotations.py --time data` checks and summarises every annotation
file of a directory.

## Sizes and load times

Four plants per row, `--annotations skeleton,labels,points=4096,branches,json`, mean
sizes; load times are medians of 20 loads of one plant from a warm disk cache with
NumPy 2 on one desktop, a record of one machine and not a target.

| Plant | Nodes | Vertices | `.glb` | `.ann.npz` | `.ann.json` | `.points.ply` | load `.npz` | load `.json` | load `.ply` |
|---|---|---|---|---|---|---|---|---|---|
| `thicket_shrub`, age 10 | 652 | 21 121 | 1.3 MB | 0.20 MB | 0.45 MB | 0.12 MB | 1.0 ms | 5.5 ms | 0.1 ms |
| `broadleaf`, age 20 | 1 176 | 101 132 | 5.8 MB | 0.62 MB | 1.2 MB | 0.12 MB | 1.5 ms | 18 ms | 0.1 ms |
| `excurrent_conifer`, age 20 | 2 307 | 172 236 | 9.7 MB | 1.1 MB | 2.2 MB | 0.12 MB | 1.7 ms | 33 ms | 0.1 ms |
| `oak_like`, age 40 | 2 997 | 180 550 | 9.4 MB | 1.0 MB | 1.9 MB | 0.12 MB | 1.6 ms | 34 ms | 0.1 ms |

The per-vertex labels (5 bytes per vertex) are most of the `.npz`; the skeleton and the
branch table of a plant of a few thousand nodes take about 100 kB. The `.npz` is not
compressed so that loading stays a copy; the labels compress well if storage matters.

## Realism metric v0

`arbrume-metrics` computes the geometric feature set of ICTree (Polasek et al. 2021) on
the live skeleton. Their user study and feature analysis found the sibling angles and the
segment volume the features that move perceived realism most, and chain deformation and
segment count the least. Only the features are computed: the trained ICTree predictor is
not used, so the numbers are a **regression baseline** for each species, to see a change
of a species or of the engine, not a verdict on realism.

ICTree cuts the skeleton at its branching nodes: a *chain* there runs from the root or a
branching node to the next branching node or a tip, so an axis that bears laterals is
several chains. The features, each summarised per plant by count, mean, variance,
minimum and maximum:

| Group | Features |
|---|---|
| segment (internode) | `segment_width` (diameter), `segment_volume` (frustum between the radius of the previous internode of the axis and its own) |
| chain | `chain_segments`, `chain_depth` (1 at the root), `chain_length` `l`, `chain_total_length` `L` (chord), `chain_deformation` (sum of turning angles), `chain_straightness` `L / l`, `chain_slope` (first segment to +Y), `chain_width_min`, `chain_width_max`, `chain_tapering` (min / max), `chain_internodes` (`l` over the mean chain length, mean 1 by construction), `chain_tropism_b` (first to last segment), `chain_tropism_h` (last segment to +Y), `chain_tropism_p` (last segment to chord) |
| sibling pair | `sibling_angle` (first segments), `sibling_total_angle` (last segments), and per branching node their smallest and largest |
| parent and child | `parent_length_ratio`, `parent_angle_sum` (difference of deformations), `parent_continuation` (last segment of the parent to first of the child), `parent_preservation` (chord to chord) |
| branching | `asymmetry_tips`, `asymmetry_length`, `asymmetry_volume`: `|a - b| / (a + b)` over the subtrees of each sibling pair |
| global | `internode_mean`, `internode_var` (the paper's estimate: mean and variance of the chain length), `trunk_length`, `tip_count`, `age` (`trunk_length / internode_mean + max depth - 1`), and, not in the paper, `segment_count`, `chain_count`, `total_volume`, `segment_length_mean` |

Angles are in degrees, lengths in meters, volumes in cubic meters. Where the paper is
ambiguous the reading taken is documented in the crate: it prints the horizontal tropism
as the angle of the first segment to the vertical, the same as the slope, so the last
segment is used; it leaves the normalisation of the asymmetry unspecified, so the sum of
the two subtrees is used. The histograms, the branching-model and ramification-model
fitness and the rendered volume imprint of the paper are not computed.

`arbrume metrics DIR` prints, per species, the mean and standard deviation over plants of
a few features from the skeletons of the `.ann.npz` files of a directory; `arbrume
metrics --presets all --seeds 64` grows the plants itself; `--json` prints every feature.
`arbrume info species.toml` prints the main features of its default plant.

### Baseline

Produced with `arbrume metrics --presets all --seeds 64 --seed 1`: plants 0 to 63 of base
seed 1 for every preset, at the middle of its `variation.age` range, scale 1, preset
parameters without jitter. Each cell is the mean ± standard deviation over the 64 plants
of the per-plant mean of a local feature, or of a global feature. A preset with no
branching node (the herbs) has no sibling or parent pair and shows 0.

| preset | age | plants | sibling angle deg | segment volume m3 | chain length m | length ratio | slope deg | tapering | straightness | tip asymmetry | tips | estimated age |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| `acacia_like` | 34 | 64 | 56.2 ± 0.352 | 3.46e-4 ± 2.95e-5 | 0.654 ± 0.0114 | 1.52 ± 0.0214 | 79.1 ± 1.62 | 1.00 ± 0 | 0.991 ± 6.48e-4 | 0.388 ± 6.31e-3 | 680 ± 231 | 26.0 ± 1.94 |
| `bamboo_like` | 6 | 64 | 68.8 ± 0.625 | 3.54e-5 ± 3.87e-6 | 0.649 ± 9.21e-3 | 1.87 ± 0.0229 | 36.8 ± 0.689 | 0.618 ± 1.68e-3 | 0.992 ± 2.06e-4 | 0.469 ± 5.06e-3 | 199 ± 6.06 | 19.9 ± 0.546 |
| `birch_like` | 27 | 64 | 60.7 ± 0.442 | 1.50e-4 ± 2.32e-5 | 0.534 ± 0.0115 | 1.74 ± 0.0435 | 81.6 ± 1.74 | 0.563 ± 8.20e-3 | 0.982 ± 1.20e-3 | 0.343 ± 9.09e-3 | 374 ± 54.5 | 29.8 ± 2.23 |
| `broadleaf` | 33 | 64 | 52.6 ± 0.368 | 3.96e-4 ± 3.59e-5 | 0.442 ± 7.56e-3 | 1.30 ± 0.0186 | 73.5 ± 1.61 | 0.584 ± 7.37e-3 | 0.990 ± 6.60e-4 | 0.314 ± 5.35e-3 | 546 ± 56.3 | 35.9 ± 2.27 |
| `conifer` | 30 | 64 | 99.1 ± 0.189 | 1.72e-4 ± 3.08e-6 | 1.78 ± 0.0209 | 4.37 ± 0.0326 | 70.1 ± 0.246 | 0.248 ± 4.62e-4 | 0.999 ± 7.84e-5 | 0.305 ± 0 | 141 ± 0 | 28.4 ± 4.98e-3 |
| `cypress_like` | 25 | 64 | 27.8 ± 0.262 | 1.42e-4 ± 7.43e-6 | 0.462 ± 7.98e-3 | 1.79 ± 0.0370 | 20.6 ± 0.352 | 0.548 ± 8.16e-3 | 0.998 ± 1.58e-4 | 0.506 ± 7.08e-3 | 288 ± 15.2 | 53.9 ± 2.42 |
| `envelope_tree` | 17 | 64 | 53.3 ± 0.450 | 1.29e-4 ± 4.25e-6 | 0.486 ± 0.0121 | 1.33 ± 0.0342 | 65.6 ± 2.08 | 1.00 ± 0 | 0.994 ± 9.44e-4 | 0.269 ± 0.0147 | 421 ± 50.3 | 23.7 ± 1.71 |
| `excurrent_conifer` | 23 | 64 | 67.9 ± 0.513 | 1.00e-4 ± 3.04e-6 | 0.480 ± 0.0112 | 1.11 ± 0.0259 | 64.2 ± 0.498 | 0.998 ± 7.88e-4 | 0.997 ± 2.82e-4 | 0.405 ± 7.13e-3 | 579 ± 25.9 | 23.1 ± 0.0260 |
| `fern_like` | 3 | 64 | 0 ± 0 | 1.37e-6 ± 9.26e-8 | 0.998 ± 0.0352 | 0 ± 0 | 24.2 ± 1.93 | 0.207 ± 2.53e-4 | 0.961 ± 1.04e-3 | 0 ± 0 | 16.0 ± 0 | 1.22 ± 0.0425 |
| `forest_grown_broadleaf` | 30 | 64 | 41.1 ± 0.274 | 9.09e-4 ± 5.04e-5 | 0.622 ± 0.0147 | 1.18 ± 0.0218 | 47.9 ± 1.03 | 0.977 ± 2.31e-3 | 0.997 ± 3.49e-4 | 0.242 ± 7.67e-3 | 776 ± 95.4 | 35.9 ± 2.61 |
| `hedge_shrub` | 7 | 64 | 50.0 ± 0.980 | 2.33e-6 ± 5.88e-8 | 0.121 ± 3.60e-3 | 1.07 ± 0.0271 | 43.3 ± 2.07 | 1.00 ± 0 | 0.998 ± 4.51e-4 | 0.172 ± 0.0164 | 212 ± 29.5 | 10.3 ± 0.785 |
| `larch_like` | 32 | 64 | 98.3 ± 0.426 | 3.52e-4 ± 9.25e-6 | 2.57 ± 0.0411 | 5.55 ± 0.0824 | 69.1 ± 0.533 | 0.235 ± 3.50e-3 | 0.994 ± 3.06e-4 | 0.325 ± 4.72e-3 | 132 ± 3.34 | 29.6 ± 9.27e-3 |
| `maple_like` | 34 | 64 | 51.9 ± 0.447 | 3.52e-4 ± 2.30e-5 | 0.471 ± 9.89e-3 | 1.31 ± 0.0211 | 68.3 ± 2.20 | 1.00 ± 0 | 0.993 ± 7.29e-4 | 0.339 ± 9.01e-3 | 519 ± 108 | 27.2 ± 2.10 |
| `oak_like` | 50 | 64 | 56.9 ± 0.353 | 5.72e-4 ± 4.70e-5 | 0.591 ± 0.0117 | 1.37 ± 0.0256 | 72.8 ± 2.30 | 1.00 ± 0 | 0.991 ± 7.52e-4 | 0.343 ± 9.99e-3 | 726 ± 207 | 28.2 ± 1.87 |
| `open_grown_broadleaf` | 17 | 64 | 46.0 ± 0.368 | 2.99e-4 ± 8.07e-6 | 0.558 ± 9.18e-3 | 1.19 ± 0.0183 | 51.5 ± 1.47 | 0.990 ± 1.91e-3 | 0.999 ± 1.53e-4 | 0.258 ± 9.32e-3 | 603 ± 104 | 23.7 ± 1.54 |
| `palm_like` | 18 | 64 | 74.6 ± 0.949 | 2.16e-4 ± 3.93e-6 | 1.66 ± 0.0209 | 20.4 ± 0.282 | 37.0 ± 0.563 | 0.560 ± 2.26e-4 | 0.965 ± 4.14e-4 | 0.735 ± 0 | 21.0 ± 0 | 22.1 ± 0.0269 |
| `pine_like` | 33 | 64 | 71.8 ± 0.475 | 2.93e-4 ± 8.15e-6 | 1.01 ± 0.0167 | 3.39 ± 0.0824 | 64.5 ± 1.03 | 0.493 ± 4.73e-3 | 0.989 ± 3.42e-4 | 0.450 ± 4.56e-3 | 942 ± 53.5 | 28.3 ± 0.0883 |
| `poplar_like` | 25 | 64 | 32.4 ± 0.186 | 3.04e-4 ± 3.63e-5 | 0.406 ± 6.19e-3 | 1.33 ± 0.0192 | 28.9 ± 0.475 | 0.575 ± 7.26e-3 | 0.997 ± 2.56e-4 | 0.343 ± 6.58e-3 | 690 ± 95.6 | 33.8 ± 2.94 |
| `rosette_herb` | 1 | 64 | 0 ± 0 | 1.60e-6 ± 1.65e-7 | 0.170 ± 6.26e-3 | 0 ± 0 | 71.9 ± 2.79 | 1.00 ± 0 | 1.00 ± 0 | 0 ± 0 | 30.0 ± 0 | 1.33 ± 0.0471 |
| `sapling` | 5 | 64 | 54.5 ± 2.88 | 1.08e-5 ± 3.67e-6 | 0.321 ± 0.117 | 1.85 ± 0.414 | 44.6 ± 5.92 | 0.549 ± 0.0405 | 0.999 ± 7.31e-4 | 0.275 ± 0.0755 | 14.4 ± 5.30 | 8.19 ± 2.10 |
| `shrub` | 9 | 64 | 54.0 ± 0.654 | 5.51e-6 ± 6.29e-7 | 0.242 ± 5.01e-3 | 1.86 ± 0.0491 | 56.6 ± 1.52 | 0.566 ± 5.49e-3 | 0.992 ± 7.59e-4 | 0.375 ± 0.0128 | 311 ± 32.5 | 16.2 ± 1.19 |
| `spreading_decurrent` | 20 | 64 | 54.0 ± 0.425 | 5.21e-4 ± 2.51e-5 | 0.587 ± 0.0195 | 1.22 ± 0.0355 | 68.7 ± 2.00 | 0.975 ± 3.54e-3 | 0.995 ± 6.43e-4 | 0.234 ± 0.0121 | 478 ± 75.9 | 22.5 ± 1.79 |
| `spreading_shrub` | 8 | 64 | 66.4 ± 0.690 | 2.87e-6 ± 2.90e-7 | 0.175 ± 2.41e-3 | 1.45 ± 0.0217 | 75.2 ± 1.15 | 0.592 ± 8.50e-3 | 0.994 ± 5.49e-4 | 0.344 ± 5.86e-3 | 717 ± 49.1 | 17.2 ± 0.835 |
| `spruce_like` | 31 | 64 | 105 ± 0.333 | 2.62e-4 ± 4.23e-6 | 2.23 ± 0.0254 | 5.26 ± 0.0424 | 70.5 ± 0.362 | 0.251 ± 4.49e-4 | 0.997 ± 1.69e-4 | 0.364 ± 4.44e-16 | 121 ± 0 | 30.2 ± 2.76e-3 |
| `thicket_shrub` | 8 | 64 | 41.4 ± 1.10 | 5.41e-6 ± 2.00e-7 | 0.201 ± 0.0101 | 1.12 ± 0.0546 | 39.0 ± 1.56 | 0.985 ± 4.21e-3 | 0.997 ± 8.25e-4 | 0.160 ± 0.0179 | 110 ± 13.9 | 9.89 ± 1.07 |
| `upright_shrub` | 10 | 64 | 49.2 ± 0.624 | 1.48e-5 ± 2.29e-6 | 0.299 ± 5.35e-3 | 1.62 ± 0.0364 | 40.7 ± 0.797 | 0.552 ± 5.86e-3 | 0.997 ± 4.32e-4 | 0.351 ± 9.39e-3 | 456 ± 41.0 | 20.1 ± 1.51 |
| `willow_like` | 37 | 64 | 52.1 ± 1.04 | 4.98e-4 ± 4.51e-5 | 1.20 ± 0.0378 | 1.92 ± 0.0484 | 113 ± 2.33 | 1.00 ± 0 | 0.978 ± 1.02e-3 | 0.338 ± 0.0104 | 435 ± 86.2 | 24.4 ± 1.74 |

Some presets give the same topology for every seed (the parametric conifers, the palm),
hence a zero spread of the tip count. Compare a new version against this table preset by
preset: a shift of several standard deviations in a feature that the change was not meant
to touch is worth a look.

## Throughput

`arbrume bench --species broadleaf,excurrent_conifer,thicket_shrub,oak_like --ages 10,20
--repeats 7 --batch 128`, without and with `--annotations`, on one 16-thread desktop (a
record of one machine, not a target): plants per second, generated and exported to memory
with their annotations.

| species | age | | none, 1 thread | none, 16 threads | `skeleton,labels,branches`, 1 thread | 16 threads | `skeleton,labels,points=4096,branches,metrics`, 1 thread | 16 threads |
|---|---|---|---|---|---|---|---|---|
| `broadleaf` | 10 | 11 192 | 4626 | 11 232 | 2489 | 7976 | 1335 | 5788 |
| `broadleaf` | 20 | 65 245 | 999 | 1809 | 611 | 1452 | 440 | 1155 |
| `excurrent_conifer` | 10 | 10 746 | 1564 | 5875 | 1689 | 6337 | 1112 | 5074 |
| `excurrent_conifer` | 20 | 81 434 | 265 | 766 | 244 | 643 | 200 | 573 |
| `thicket_shrub` | 10 | 17 415 | 1747 | 7304 | 1326 | 5949 | 913 | 4557 |
| `thicket_shrub` | 20 | 23 408 | 578 | 3004 | 479 | 2566 | 392 | 2247 |
| `oak_like` | 10 | 1 965 | 12 105 | 45 343 | 6698 | 34 444 | 3005 | 21 298 |
| `oak_like` | 20 | 16 916 | 3432 | 12 089 | 2296 | 12 638 | 1371 | 5236 |

The cost is linear in the size of the plant: on `broadleaf` at age 20 (101 132 vertices,
1 024 nodes), per plant, the provenance of the vertices (a second planning pass of the
mesher) takes about 0.10 ms, the label arrays 0.09 ms, their `f32` copies for the `.glb`
0.10 ms, the skeleton 0.02 ms, the branch table 0.06 ms, the `.npz` 0.13 ms, 4096 points
0.6 ms and the metrics 0.10 ms. Growing and meshing the same plant take 0.6 ms, writing
its 4.9 MB `.glb` 0.24 ms to memory. On disk the annotations add the files of
[Sizes and load times](#sizes-and-load-times), an eighth to a quarter of the bytes of
the `.glb` without the JSON copy; a batch that writes to disk is bound by the disk sooner
than by the annotations. Points are the most expensive item: ask for them only when
needed, and a few thousand per plant are enough for skeleton extraction methods.

## Library

| Item | Use |
|---|---|
| `arbrume_core::mesh::{SemanticClass, VertexRun, MeshProvenance, PartLabels}` | classes, and where each vertex of a mesh comes from |
| `arbrume_mesh::Mesher::provenance(graph, species, options)`, `mesh_provenance(..)` | the provenance of the mesh the same arguments build, from the planning pass only |
| `arbrume_gen::Generator::provenance(&plant, species)` | the provenance of the mesh of a generated plant |
| `arbrume_export::annotations::{Skeleton, BranchTable, MeshLabels, GlbLabels, Annotations}` | the sidecar content, `Annotations::write_npz`, `to_json`, `read_npz`, `Skeleton::to_graph` |
| `arbrume_export::annotations::{sample_points, PointCloud}` | point sampling, `PointCloud::write_ply` |
| `arbrume_export::{write_glb_with, ExtraAttribute, AttributeData}` | `.glb` with custom vertex attributes |
| `arbrume_metrics::{features, Features, Summary, LOCAL_FEATURES, GLOBAL_FEATURES}` | the realism features of a `PlantGraph` |

## Limits

* Classes are structural: a palm frond is a branch with leaves, a fern frond likewise.
  Flowers and fruits are not meshed yet, so no class exists for them.
* The bark of an axis dropped by `min_radius` is absent from the mesh, but the axis stays
  in the skeleton and the branch table (they describe the graph, not the mesh).
* The point sampler models no scanner: no occlusion, noise or density fall-off.
* The metric is the feature set only; turning it into a realism score needs a predictor
  trained on human judgements, which is not part of Arbrume.
