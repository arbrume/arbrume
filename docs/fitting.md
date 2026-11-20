# Parameter fitting

`arbrume fit` starts from a tree skeleton, measured, scanned or generated, and a species
preset, and searches the species parameters, the age and the scale with which the preset
grows look-alikes of that skeleton. The result is a species file: the preset with the
fitted values and a `[variation]` table, ready for `arbrume gen`. It runs offline, on the
CPU, in seconds for the presets of the library. The library is the `arbrume-fit` crate.

```sh
arbrume fit --target scan.csv --from oak_like --out my_oak.toml --report my_oak.json
arbrume fit --target plants/maple_like_1_0.ann.npz --from oak_like --seeds 4 --budget 1200
arbrume fit --target scan.csv --from oak_like --up z --min-radius 0.02 --smooth 2
arbrume fit --target scan.csv --from oak_like --out my_oak.toml --resume
arbrume distance a.ann.npz b.csv --min-radius 0.01
```

The method follows Stava et al. (2014), who fitted a developmental tree model to input
trees with a distance made of a shape, a geometry and a structure term and a Markov chain
Monte Carlo search with simulated annealing. Here the distance has the same three groups,
with terms chosen so that a target needs no age and no branching orders, and the search is
the CMA-ES evolution strategy of Hansen (2016).

## Inputs

| File | Read as |
|---|---|
| `<stem>.ann.npz` | the skeleton sidecar of `arbrume gen --annotations skeleton` ([ML annotations](annotations.md)); shed nodes are left out |
| `<stem>.glb` written by Arbrume | its `<stem>.ann.npz` when that file is next to it, else the plant grown again from the metadata in the `extras` of the file (species, seed, age, scale; a plant grown in surroundings needs its sidecar) |
| `.csv` | one node per line (format below) |
| `.json` | `{"position": [[x, y, z], ...], "radius": [r, ...], "parent": [p, ...]}`, parents as indices into the arrays, `-1` for a root |

The CSV form is meant for scans and other tools. A header names the columns, in any
order; other columns are ignored; lines starting with `#` are comments:

```text
# x y z in meters; radius of the segment from the parent to the node
id,x,y,z,radius,parent
0,0.0,0.0,0.0,0.21,
1,0.0,1.1,0.0,0.19,0
2,0.02,2.3,0.0,0.16,1
3,0.6,2.0,0.1,0.05,1
```

`id` is any text unique per line; `parent` is the id of the parent node, empty or `-1` for
a root (a multi-stemmed plant has several). A root's radius is that of the base of its
stem. Positions are meters, Y up; `--up z` reads a Z-up file (`(x, y, z)` becomes
`(x, z, -y)`), as most scans are. Nothing else is expected: no age, no order, no
internode spacing.

Scan-to-skeleton tools such as AdTree (Du et al. 2019) or the method of Livny et al.
(2010) output a graph of points with radii. The AdTree paper describes its pipeline but
not the layout of its output files, so Arbrume does not read them directly: write their
vertices, radii and edges to the CSV form above (one line per vertex, the parent being
the neighbour towards the root). Such skeletons are noisy and incomplete at the fine
branches (Livny et al. 2010; Du et al. 2019 report their largest errors at the tips),
which is what `--min-radius` and `--smooth` are for.

## The distance

`arbrume distance A B` and `arbrume_fit::distance` compare two skeletons. Both are first
normalised the same way:

1. **Pruning.** A node is kept when its radius and the radii of all its ancestors are at
   least `--min-radius` (default 0, nothing pruned). Use it to compare a scan whose small
   branches are unreliable: the candidates are pruned the same way.
2. **Placement.** The base (the mean of the roots in X and Z, their lowest Y) is moved
   to the origin. The terms below are invariant to a rotation about the vertical, so the
   azimuth of a scan does not matter.
3. **Axes.** Each root starts an axis of order 0. At each node the axis continues into
   the child that carries the most wood (the largest total length of its subtree; ties go
   to the straightest continuation, then to the position), and the other children start
   axes of the next order. Orders thus come from the geometry alone, the same way for a
   scan and for an Arbrume plant.
4. **Resampling.** Every axis is cut into `max(1, round(L / step))` equal segments, each
   with the radius of the input segment under its middle; a lateral is attached to the
   nearest resampled node of its parent. The step is `--step`, by default the height of
   the taller tree over 100 (in a fit, the height of the target over 100). This removes
   the dependence of the terms on the node spacing of the input. `--smooth N` then
   applies `N` passes of a `[1/4, 1/2, 1/4]` filter to the inner points of every axis,
   against position noise.
5. The result is a plant graph, from which the `ICTree` features of
   [the realism metric](annotations.md#realism-metric-v0) are computed.

The terms, each in `[0, 1]`, symmetric in the two trees, with `H` and `R` the largest
height and horizontal reach of the two and the resampled segments as samples weighted by
their length:

| Group | Term | Definition |
|---|---|---|
| shape | `envelope` | per height bin (12 over `[0, H]`), the largest horizontal distance of wood from the vertical through the base: a radial profile, the crown silhouette swept around the trunk. One minus the intersection over union of the two profiles. |
| shape | `density` | the share of the wood length in each of 12 by 6 cells of `[0, H] x [0, R]`; the total variation distance between the two. |
| geometry | `geometry` | per order (0, 1, 2, 3 and deeper), the distributions over the axes of the length, the base radius, the insertion angle (to the parent segment, or to the vertical for a stem) and the inclination of the chord to the vertical; compared with the 1-Wasserstein distance divided by the sum of the two means (lengths, radii) or by 180 degrees (angles). A distribution present in one tree only counts 1. The mean over the pairs present in either tree. |
| structure | `counts` | per order, `abs(a - b) / (a + b)` of the numbers of axes; the mean. |
| structure | `features` | the plant means of every `ICTree` local feature but `chain_internodes` (1 by construction) and the global features `trunk_length`, `tip_count`, `chain_count` and `total_volume`: `abs(a - b) / 180` for angles, `abs(a - b) / (abs(a) + abs(b))` for the others; the mean. |

The total is the weighted mean of the terms. The default weights, envelope 0.5,
density 0.5, geometry 1, counts 0.5, features 0.5, give the three groups equal weight, as
Stava et al. did in all their results; they are a choice, not a calibration
(`arbrume_fit::Weights`). Stava et al. compare structure with a tree edit distance; the
counts and features used here are cheaper and do not need the two trees to be matched
node to node.

The distance is 0 between a skeleton and itself, symmetric, does not depend on the order
of the nodes, and grows with a perturbation (the tests check these). The distance between
two seeds of the same preset is not 0: a fit cannot do better than that floor.

### Separation

How well do the terms tell presets apart? Four plants of each of eight presets
(`broadleaf`, `maple_like`, `oak_like`, `birch_like`, `poplar_like`, `willow_like`,
`pine_like`, `spruce_like`) at the middle of their age range, mean term over the pairs of
plants of the same preset and over the pairs of different presets
(`cargo run --release -p arbrume-fit --example fit_validate -- separation`):

| term | within a preset | between presets | ratio |
|---|---|---|---|
| envelope | 0.1235 | 0.4710 | 3.81 |
| density | 0.1764 | 0.5042 | 2.86 |
| geometry | 0.0388 | 0.2992 | 7.71 |
| counts | 0.0651 | 0.3375 | 5.19 |
| features | 0.0177 | 0.1866 | 10.56 |
| total | 0.0767 | 0.3496 | 4.56 |

Total with other bin counts (height by radial):

| bins | within | between | ratio |
|---|---|---|---|
| 24 by 12 | 0.0886 | 0.3583 | 4.05 |
| 12 by 6 | 0.0767 | 0.3496 | 4.56 |
| 8 by 4 | 0.0713 | 0.3445 | 4.83 |

The bin counts were chosen from this table: coarser bins separate presets better, finer
bins resolve more of the crown; 12 by 6 is the middle of the three settings measured.

### Speed

Microseconds to normalise one skeleton (`Descriptor::new`: prune, axes, resampling,
features) and to compare two normalised skeletons (`Descriptor::compare`), one thread,
plants at the oldest age of their range. A fit normalises each target once and each
candidate plant once.

| preset | age | nodes | resampled nodes | normalise us | compare us |
|---|---|---|---|---|---|
| sapling | 8 | 328 | 639 | 153 | 4.1 |
| broadleaf | 50 | 3029 | 3433 | 1329 | 34.4 |
| maple_like | 50 | 5103 | 6781 | 2163 | 56.1 |
| oak_like | 70 | 6882 | 10722 | 2670 | 89.5 |
| pine_like | 52 | 5805 | 7372 | 1704 | 66.0 |

## The search

`arbrume fit` minimises the mean distance from the target to the plants that a point of
the search space grows with `--seeds K` plant seeds (default 4). The same `K` seeds serve
every candidate (common random numbers), so two candidates are compared on the same
draws and the objective is a deterministic function of the parameters; the report then
measures the result on 16 other seeds (`--holdout`), which the search never used.

**Space.** Each fitted jitterable parameter (the list `JITTERABLE` of
[the species variation](species.md#variation)) is multiplied by `exp(x)`, the same factor
on every order of a curve, clamped to its domain as a jitter is
(`Species::with_factor`); the age is `round(base_age * exp(x))`, where `base_age` is the
middle of the `variation.age` range of the base species; the scale is `exp(x)`. Every
coordinate is searched within `[-ln F, ln F]` of its start, `F = --max-factor`
(default 3). A parameter that is 0 in the base species stays 0. Without `--params`, the
fitted parameters are the jitterable ones that change the skeleton of the base species
(each is probed with factors 0.8 and 1.25 on one plant: colours, leaf sizes, unused
self-organization parameters and parameters at 0 are dropped and listed in the report),
plus age and scale.

**Start.** The base species at `base_age`, with the scale that brings the mean height of
its `K` plants to the height of the target. The base species itself (scale 1) and this
start point are evaluated first.

**Strategy.** The `(mu/mu_w, lambda)`-CMA-ES of Hansen's tutorial (2016) with its default
population `lambda = 4 + floor(3 ln n)`, weights, learning rates and step-size damping,
implemented in `arbrume_fit::Cmaes` (no external solver); initial step `0.3 ln F`.
Candidates outside the box are evaluated at the nearest point inside and pay their
squared distance to it. Each generation grows its `lambda * K` plants with
`arbrume_gen::run_batch` on all threads, without meshing them.

**Stop.** When the next generation would exceed `--budget` candidates (default 1200),
when the best distance has not improved by more than 1e-4 for 30 generations, or when the
search has collapsed (largest standard deviation below 1e-3).

**Determinism.** Sampling uses the counter-based generator of `arbrume-core` keyed by the
fit seed (`--seed`), the generation and the candidate; the plants are pure functions of
their parameters and seeds, and results are combined in index order. A fit therefore
gives the same species file whatever `--jobs`. After every generation the whole state
(strategy, archive of every candidate, history) is written as JSON to the checkpoint
(`--checkpoint`, by default `<out>.checkpoint.json`); `--resume` continues from it and
ends with the same result as an uninterrupted run (floats are written so that they read
back exactly). The checkpoint records a hash of the target, the base species and the
settings, and refuses to resume another fit.

## Outputs

**Species file** (`--out`, default `fitted.toml`): the base species file with the fitted
parameters replaced (six significant digits), `name` suffixed with `(fitted)`, and a
`[variation]` fitted from the spread of the good candidates: those whose distance is
within one seed-to-seed standard deviation of the best (the standard deviation over its
`K` seeds of the best candidate's distance), which `K` plants cannot tell from the best.
`variation.age` and `variation.scale` are the range of their ages and scales; each fitted
parameter gets a jitter of `sqrt(3)` times the standard deviation of their factors
relative to the best (the half-width of a uniform law with that deviation), at most 0.9.
Jitters of parameters that were not fitted are kept. Two comment lines at the top give
the fitted age and scale and the held-out distances.

**Report** (`--report`, JSON): `target` (height, reach, resampled nodes, axes per order),
`step`, `parameters` and `dropped_parameters`, `options`, the `seeds`, and the terms of
`before` (the base species at `base_age`, scale 1), `start`, `after` (the best
candidate) and `search_mean` (the final mean of the strategy), each on the training seeds
and on the held-out seeds with their seed-to-seed spread; `fitted` (age, scale, factors),
`variation`, `generations`, `evaluations`, `growths`, `stopped` (`budget`,
`no improvement` or `converged`), `seconds` and the per-generation `history`.

From Python ([Python bindings](python.md)):

```python
import arbrume

terms = arbrume.distance("scan.csv", tree, min_radius=0.02, up="z")
result = arbrume.fit("scan.csv", "oak_like", up="z", min_radius=0.02, budget=600)
open("my_oak.toml", "w").write(result["toml"])
print(result["age"], result["scale"], result["report"]["after"]["holdout"]["total"])
```

`fit` and `distance` accept a `Tree`, a path or a dict of `position`, `radius` and
`parent` arrays; they release the GIL.

## Validation

Every number below comes from
`cargo run --release -p arbrume-fit --example fit_validate -- <run> OUT_DIR` on one
16-thread desktop shared with other jobs (wall times are a record of that machine, not a
target). Distances are the mean total over 16 held-out seeds; "before" is the base preset
at the middle of its age range and scale 1. For reference, the mean distance between two
plants of the same preset is about 0.077 (separation table): a fit at that level is as
close as another seed of the target's own species.

Each run also writes one contact sheet per fit, `<target>_from_<preset>.png`: row 1 the
target (first its skeleton as the distance sees it, meshed as bark, then two more plants
of the target species), row 2 the base preset, row 3 the fitted species, all bark only,
then the target species and the fitted species with foliage; three seeds per row, one
camera for the whole sheet, so sizes compare.

### Self-recovery

Twelve targets, each a preset with `growth.axis_length`, `growth.branching_angle`,
`growth.gravitropic_setpoint` and `growth.branch_probability` multiplied by random
factors in `[e^-0.35, e^0.35]`, at a random age of its range and a scale in
`[e^-0.2, e^0.2]`. Each is fitted three ways: from its own preset with only those four
parameters, age and scale (`4+2`, a well-posed problem), from its own preset with the
default parameters, and from another preset. *Param error* is the mean over the four
parameters of `abs(ln(fitted / true))`, each parameter summarised by its mean over orders
0 to 2 and positions 0.1 to 0.9 (0.35 is the error of a factor `e^0.35`); *length error*
is the same for the axis length times the scale.

| target | from | fitted | before | after | param error before | after | length error before | after | age true/fit | scale true/fit | s |
|---|---|---|---|---|---|---|---|---|---|---|---|
| broadleaf_0 | broadleaf | 4+2 | 0.1732 | 0.1072 | 0.166 | 0.484 | 0.081 | 0.046 | 33/20 | 1.12/1.81 | 5.1 |
| broadleaf_0 | broadleaf | default | 0.1732 | 0.1218 | 0.166 | 0.117 | 0.081 | 0.108 | 33/30 | 1.12/1.15 | 2.2 |
| broadleaf_0 | maple_like | default | 0.2307 | 0.1946 | 0.055 | 0.230 | 0.093 | 0.281 | 33/35 | 1.12/1.56 | 3.4 |
| maple_like_1 | maple_like | 4+2 | 0.2805 | 0.1402 | 0.253 | 0.204 | 0.326 | 0.357 | 34/30 | 0.87/0.85 | 3.9 |
| maple_like_1 | maple_like | default | 0.2805 | 0.1267 | 0.253 | 0.210 | 0.326 | 0.289 | 34/58 | 0.87/0.87 | 2.1 |
| maple_like_1 | broadleaf | default | 0.2759 | 0.1490 | 0.361 | 0.460 | 0.500 | 0.615 | 34/38 | 0.87/0.79 | 1.4 |
| oak_like_2 | oak_like | 4+2 | 0.2740 | 0.1258 | 0.205 | 0.241 | 0.220 | 0.224 | 35/48 | 0.92/0.66 | 1.6 |
| oak_like_2 | oak_like | default | 0.2740 | 0.0896 | 0.205 | 0.265 | 0.220 | 0.240 | 35/40 | 0.92/0.92 | 1.4 |
| oak_like_2 | maple_like | default | 0.2328 | 0.1052 | 0.229 | 0.293 | 0.268 | 0.098 | 35/29 | 0.92/1.07 | 2.0 |
| birch_like_3 | birch_like | 4+2 | 0.3221 | 0.0785 | 0.220 | 0.121 | 0.140 | 0.064 | 16/18 | 0.84/0.74 | 1.5 |
| birch_like_3 | birch_like | default | 0.3221 | 0.0972 | 0.220 | 0.139 | 0.140 | 0.121 | 16/22 | 0.84/0.68 | 1.2 |
| birch_like_3 | broadleaf | default | 0.3698 | 0.1501 | 0.250 | 0.274 | 0.154 | 0.274 | 16/27 | 0.84/0.81 | 0.7 |
| poplar_like_4 | poplar_like | 4+2 | 0.2785 | 0.0954 | 0.125 | 0.252 | 0.167 | 0.135 | 17/21 | 0.90/0.70 | 0.7 |
| poplar_like_4 | poplar_like | default | 0.2785 | 0.1022 | 0.125 | 0.230 | 0.167 | 0.078 | 17/27 | 0.90/0.93 | 1.0 |
| poplar_like_4 | birch_like | default | 0.3423 | 0.1463 | 0.898 | 1.193 | 0.126 | 0.755 | 17/26 | 0.90/0.72 | 0.8 |
| willow_like_5 | willow_like | 4+2 | 0.1688 | 0.1062 | 0.146 | 0.324 | 0.401 | 0.308 | 24/20 | 1.17/1.52 | 0.6 |
| willow_like_5 | willow_like | default | 0.1688 | 0.1194 | 0.146 | 0.252 | 0.401 | 0.040 | 24/38 | 1.17/0.81 | 1.9 |
| willow_like_5 | maple_like | default | 0.2416 | 0.1690 | 0.228 | 0.159 | 0.404 | 0.095 | 24/49 | 1.17/1.04 | 0.5 |
| pine_like_6 | pine_like | 4+2 | 0.2583 | 0.0389 | 0.291 | 0.065 | 0.130 | 0.002 | 38/39 | 1.20/1.16 | 1.9 |
| pine_like_6 | pine_like | default | 0.2583 | 0.1121 | 0.291 | 0.381 | 0.130 | 0.809 | 38/37 | 1.20/1.16 | 1.2 |
| pine_like_6 | spruce_like | default | 0.5422 | 0.4067 | 0.397 | 0.087 | 0.249 | 0.609 | 38/21 | 1.20/1.97 | 0.2 |
| spruce_like_7 | spruce_like | 4+2 | 0.0828 | 0.0225 | 0.133 | 0.011 | 0.077 | 0.014 | 32/32 | 1.16/1.16 | 0.6 |
| spruce_like_7 | spruce_like | default | 0.0828 | 0.0354 | 0.133 | 0.297 | 0.077 | 0.041 | 32/31 | 1.16/0.74 | 0.6 |
| spruce_like_7 | conifer | default | 0.1682 | 0.0378 | 0.182 | 0.239 | 0.224 | 0.068 | 32/35 | 1.16/2.00 | 0.7 |
| conifer_8 | conifer | 4+2 | 0.1240 | 0.0253 | 0.100 | 0.063 | 0.137 | 0.022 | 48/51 | 0.98/0.98 | 0.8 |
| conifer_8 | conifer | default | 0.1240 | 0.0687 | 0.100 | 0.195 | 0.137 | 0.039 | 48/33 | 0.98/1.36 | 0.2 |
| conifer_8 | spruce_like | default | 0.2014 | 0.0625 | 0.092 | 0.187 | 0.284 | 0.031 | 48/78 | 0.98/0.68 | 0.9 |
| acacia_like_9 | acacia_like | 4+2 | 0.1693 | 0.0949 | 0.198 | 0.095 | 0.081 | 0.042 | 39/38 | 0.84/0.91 | 8.1 |
| acacia_like_9 | acacia_like | default | 0.1693 | 0.0786 | 0.198 | 0.068 | 0.081 | 0.042 | 39/17 | 0.84/0.86 | 8.2 |
| acacia_like_9 | oak_like | default | 0.2189 | 0.1114 | 0.170 | 0.200 | 0.070 | 0.370 | 39/62 | 0.84/0.64 | 9.7 |
| shrub_10 | shrub | 4+2 | 0.2712 | 0.0980 | 0.127 | 0.121 | 0.341 | 0.018 | 10/10 | 1.00/1.00 | 0.4 |
| shrub_10 | shrub | default | 0.2712 | 0.0886 | 0.127 | 0.467 | 0.341 | 0.054 | 10/10 | 1.00/1.77 | 0.8 |
| shrub_10 | upright_shrub | default | 0.4467 | 0.1468 | 0.544 | 0.558 | 0.985 | 0.105 | 10/6 | 1.00/0.51 | 0.7 |
| sapling_11 | sapling | 4+2 | 0.4845 | 0.1819 | 0.205 | 0.507 | 0.049 | 0.171 | 3/2 | 1.21/1.72 | 0.0 |
| sapling_11 | sapling | default | 0.4845 | 0.2858 | 0.205 | 0.272 | 0.049 | 0.036 | 3/4 | 1.21/1.19 | 0.1 |
| sapling_11 | broadleaf | default | 0.6980 | 0.2275 | 0.720 | 0.566 | 1.174 | 0.946 | 3/90 | 1.21/0.36 | 0.3 |

Summary:

| fit | fits | median before | median after | after < before | median param error before | after | param error improved | median length error before | after |
|---|---|---|---|---|---|---|---|---|---|
| own preset, 4+2 | 12 | 0.2647 | 0.0967 | 12/12 | 0.182 | 0.162 | 7/12 | 0.139 | 0.055 |
| own preset, default | 12 | 0.2647 | 0.0997 | 12/12 | 0.182 | 0.241 | 4/12 | 0.139 | 0.066 |
| other preset, default | 12 | 0.2587 | 0.1479 | 12/12 | 0.239 | 0.257 | 3/12 | 0.259 | 0.278 |

### Cross-preset

Unperturbed plants of one preset at the middle of its age range, fitted from another
preset with the default settings.

| target | from | before | after | after, train | age | scale | candidates | s |
|---|---|---|---|---|---|---|---|---|
| maple_like_plain0 | oak_like | 0.1519 | 0.1182 | 0.0856 | 126 | 0.85 | 1190 | 2.5 |
| oak_like_plain1 | maple_like | 0.1497 | 0.1121 | 0.0932 | 54 | 1.09 | 1157 | 2.8 |
| birch_like_plain2 | poplar_like | 0.2975 | 0.1089 | 0.0986 | 30 | 0.78 | 1190 | 1.1 |
| willow_like_plain3 | broadleaf | 0.2441 | 0.1648 | 0.1502 | 77 | 0.87 | 1190 | 3.2 |
| pine_like_plain4 | conifer | 0.5048 | 0.3099 | 0.3070 | 27 | 2.33 | 1190 | 0.5 |
| acacia_like_plain5 | maple_like | 0.2530 | 0.1793 | 0.1442 | 49 | 0.85 | 1190 | 1.1 |

### Noisy targets

Five perturbed targets damaged as scans are: a fifth of the lateral subtrees shorter than
the median lateral are removed, and every position moves by a uniform offset of up to
3 cm along each axis. Each target is fitted from its own preset clean, damaged, and
damaged with `--min-radius` at the first quartile of the target radii and `--smooth 2`.
*After vs clean* is the mean distance, with the default settings, from 8 plants of the
fitted species to the clean target.

| target | target nodes | damaged nodes | fit on | before | after | after vs clean | age | scale | s |
|---|---|---|---|---|---|---|---|---|---|
| broadleaf_100 | 2888 | 2888 | clean | 0.1544 | 0.0435 | 0.0441 | 22/22 | 1.06/1.11 | 2.5 |
| broadleaf_100 | 2888 | 2711 | damaged | 0.1532 | 0.0516 | 0.0491 | 22/59 | 1.06/0.75 | 3.6 |
| broadleaf_100 | 2888 | 2711 | damaged, pruned and smoothed | 0.1576 | 0.0749 | 0.0733 | 22/62 | 1.06/0.67 | 2.3 |
| maple_like_101 | 1996 | 1996 | clean | 0.2493 | 0.0791 | 0.0794 | 26/61 | 0.82/0.74 | 1.0 |
| maple_like_101 | 1996 | 1890 | damaged | 0.2584 | 0.0970 | 0.0802 | 26/27 | 0.82/0.81 | 0.7 |
| maple_like_101 | 1996 | 1890 | damaged, pruned and smoothed | 0.2567 | 0.1273 | 0.1302 | 26/52 | 0.82/0.95 | 1.6 |
| oak_like_102 | 4475 | 4475 | clean | 0.1091 | 0.0967 | 0.1000 | 62/65 | 0.94/0.89 | 3.1 |
| oak_like_102 | 4475 | 4249 | damaged | 0.1145 | 0.1163 | 0.1155 | 62/82 | 0.94/0.93 | 2.9 |
| oak_like_102 | 4475 | 4249 | damaged, pruned and smoothed | 0.1159 | 0.1001 | 0.0944 | 62/49 | 0.94/0.97 | 2.5 |
| birch_like_103 | 2895 | 2895 | clean | 0.1572 | 0.0683 | 0.0693 | 26/24 | 0.93/0.81 | 1.6 |
| birch_like_103 | 2895 | 2716 | damaged | 0.1559 | 0.0967 | 0.1038 | 26/50 | 0.93/1.06 | 2.0 |
| birch_like_103 | 2895 | 2716 | damaged, pruned and smoothed | 0.1644 | 0.0928 | 0.0920 | 26/33 | 0.93/0.62 | 1.8 |
| pine_like_104 | 8203 | 8203 | clean | 0.3637 | 0.0609 | 0.0793 | 48/51 | 1.20/1.53 | 2.6 |
| pine_like_104 | 8203 | 7672 | damaged | 0.3640 | 0.0600 | 0.0631 | 48/46 | 1.20/1.14 | 2.5 |
| pine_like_104 | 8203 | 7672 | damaged, pruned and smoothed | 0.3765 | 0.0811 | 0.0601 | 48/48 | 1.20/1.07 | 2.4 |

Means over the five targets:

| fit on | after | after vs clean |
|---|---|---|
| clean | 0.0697 | 0.0744 |
| damaged | 0.0843 | 0.0823 |
| damaged, pruned and smoothed | 0.0952 | 0.0900 |

### Reading the results

* **The distance goes down in every fit**: in all 36 recovery fits and all 6 cross-preset
  fits the held-out distance after is below the distance of the base preset. Fits from the
  target's own preset with the default parameters end at a median of
  0.0997, close to the 0.077 between two seeds of one preset; fits from
  another preset end higher (median 0.1479).
* **Parameters are not recovered in general.** With the default parameter set the mean
  parameter error got smaller in 4 of 12 fits from the own preset and
  3 of 12 from another one. Even the well-posed `4+2` problem improved
  it in 7 of 12: it is recovered for the parametric conifers (`pine_like`,
  `spruce_like`, `conifer`: errors 0.065, 0.011, 0.063, age and scale within a few percent),
  but on the broadleaves age, scale and axis length trade off (`broadleaf_0`: true age 33 and
  scale 1.12, fitted 20 and 1.81, at a smaller distance than the base and a contact sheet
  that matches the target closely). The skeleton alone does not pin these parameters down;
  the fitted values are one of several that grow the same shape.
* **Training overfits the seeds.** With 4 seeds per candidate, the training distance of
  the best is below its held-out distance by 0.003 to 0.035 in the cross-preset fits (the
  best of 1 200 candidates is partly chosen for its seeds); the report gives both. Eight
  seeds per candidate narrowed that gap in a trial run without lowering the held-out
  distances consistently, at twice the time.
* **Some bases cannot reach the target.** `spruce_like` has no means to clear its bole
  into the rounded crown of `pine_like` (distance 0.54 to 0.41, contact sheet: a large
  cone); `conifer` to `pine_like` stops at 0.31. The base preset decides the habit.
* **Noise.** Dropping small branches and jittering positions by 3 cm raised the final
  distance a little (mean after 0.0697 on the clean targets, 0.0843 on the damaged ones).
  Measured against the clean targets, the plants fitted to the damaged targets are at
  0.0823, those fitted to the clean targets at 0.0744. Pruning at the first radius
  quartile with two smoothing passes did not help on these targets (0.0900 to the clean
  target): it removes a quarter of the information along with the noise. These damages
  are milder than a real scan; no real scan was tested.
* **Time.** One fit of up to 1 200 candidates by 4 seeds (4 800 plants grown and
  compared) took from under 0.1 s (`sapling`) to 10 s (`acacia_like`) on 16 threads;
  older and larger trees take longer.

## Library

| Item | Use |
|---|---|
| `TreeSkeleton`, `Up` | skeletons: `read(path, up)`, `from_csv`, `to_csv`, `from_graph`, `from_annotations` |
| `distance`, `Descriptor`, `DistanceOptions`, `Weights`, `Terms` | the distance, and normalised skeletons to compare many times |
| `normalized_graph` | a skeleton as the distance sees it, as a `PlantGraph` to mesh or inspect |
| `fit`, `fit_with_checkpoint`, `FitOptions`, `FitResult`, `Checkpoint` | the fit |
| `Cmaes`, `CmaesSettings` | the evolution strategy, usable on its own (`ask`, `tell`) |
| `Space`, `Parameter`, `Decoded`, `skeleton_parameters`, `grow_skeleton` | the search space |
| `arbrume_core::species::Species::with_factor` | a jitterable parameter multiplied by a fixed factor |

## Limits

* Only jitterable parameters, the age and the scale are fitted, by a factor on their
  base values: a parameter at 0 in the base preset stays 0, the shape of a curve along
  the axis does not change, and the qualitative traits (`architecture`, growth mode)
  come from the base preset. Pick the base preset closest to the target.
* The skeleton carries no age, and for several presets age, scale and axis length trade
  off against one another; the fitted values are one of many that look alike (see the
  recovery table), not measurements.
* The distance reads the wood only: leaf density and size are not fitted.
* The fitted `[variation]` measures how far candidates the search visited stay as close as
  the best, which reflects where the search converged as much as how the species varies;
  it is a starting point.
* There is no reader for the files of scan-to-skeleton tools; convert them to CSV.
