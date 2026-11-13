# Species library

Arbrume ships a library of species presets, embedded in `arbrume-gen` from
`crates/arbrume-gen/species/*.toml`. `arbrume species list` prints their names,
`arbrume species show <name>` prints a file, and `arbrume_gen::preset(name)` loads one.

The names are descriptive, not botanical: `oak_like` is a tree with the habit of an oak,
not a model of a particular oak. Every value in the files is a starting point chosen by
eye and from the parameter tables of Weber and Penn (1995), never a measurement. The
sizes below are those of the generated plants at the stated age, rounded.

## The presets

Triangles are those of one plant at the stated age and full quality.

| Preset | Kind | What it looks like | Age | Height | Width | Trunk | Triangles |
|---|---|---|---|---|---|---|---|
| `broadleaf` | broadleaf tree | straight trunk carried into a dense egg-shaped crown that starts low, linden-like | 40 | 15 m | 10 m | 0.45 m | 190 k |
| `maple_like` | broadleaf tree | short clear trunk, ascending scaffold, wide rounded crown of broad leaves | 40 | 13 m | 12 m | 0.4 m | 180 k |
| `oak_like` | broadleaf tree | stout trunk dividing into heavy crooked limbs, dome wider than tall | 60 | 13 m | 17 m | 0.7 m | 310 k |
| `birch_like` | broadleaf tree | slender white trunk, thin rising branches, fine pendulous twigs, light crown | 30 | 15 m | 7 m | 0.25 m | 155 k |
| `poplar_like` | broadleaf tree | columnar: branches rise close to the trunk in a tall spire | 30 | 19 m | 4.5 m | 0.4 m | 145 k |
| `willow_like` | broadleaf tree | weeping: arching limbs, long branchlets hanging in curtains of narrow leaves | 30 | 12 m | 12 m | 0.4 m | 170 k |
| `acacia_like` | broadleaf tree | umbrella: bare trunk, limbs that rise then flatten into a wide flat top | 30 | 11 m | 16 m | 0.45 m | 240 k |
| `sapling` | broadleaf tree | a young whip with a few short side branches and large leaves | 5 | 2.5 m | 1.6 m | 3 cm | 2 k |
| `conifer` | conifer | fir-like: regular whorls of level branches with flat sprays, tidy dense cone | 40 | 16 m | 6.5 m | 0.3 m | 90 k |
| `spruce_like` | conifer | whorls that sweep down and turn up at the tip, hanging branchlets, narrow cone | 40 | 18 m | 8.5 m | 0.35 m | 140 k |
| `pine_like` | conifer | cleared trunk, open whorls of upturned limbs, tufts of long needles, rounded top | 45 | 18 m | 12 m | 0.4 m | 260 k |
| `cypress_like` | conifer | columnar: short upright branches and dense dark sprays in a narrow flame | 30 | 12 m | 3 m | 0.25 m | 100 k |
| `larch_like` | conifer | level limbs, thin hanging branchlets, soft light-green needles, airy cone | 40 | 18 m | 8 m | 0.4 m | 155 k |
| `palm_like` | palm | unbranched trunk crowned by arching feather fronds, old ones drooping | 20 | 7 m | 5.5 m | 0.2 m | 27 k |
| `shrub` | shrub | rounded dense bush of many stems, small leaves | 8 | 2 m | 3 m | 4 cm | 80 k |
| `upright_shrub` | shrub | clump of tall canes rising steeply, broad leaves on short side shoots | 10 | 4.5 m | 3.5 m | 6 cm | 90 k |
| `spreading_shrub` | shrub | stems running out low over the ground, wider than tall, small dark leaves | 8 | 1.1 m | 4 m | 4 cm | 130 k |
| `bamboo_like` | cane | clump of thin green canes, bare below, whorls of twigs and narrow leaves above | 6 | 8 m | 4 m | 5 cm | 45 k |
| `fern_like` | herb | shuttlecock of arching fronds with two ranks of feathered pinnae | 2 | 0.8 m | 1.6 m | | 25 k |
| `rosette_herb` | herb | low clump of large stalked leaves from one point, hosta-like | 1 | 0.4 m | 1 m | | 0.7 k |

`broadleaf`, `conifer` and `shrub` are the generic presets; the three files of the same
names in `crates/arbrume-grow/tests/data/` are older starter species kept as test
fixtures of the growth engine.

## How the presets are built

All presets use the parametric growth mode ([Growth](growth.md)) and the foliage and bark
of the mesher ([Meshing](mesh.md)). A few devices recur:

- **Crown shape** is the `axis_length` curve of order 1 over the height of the trunk,
  with `apical_dominance` for cones, a narrow `branching_angle` and an upward
  `gravitropic_setpoint` for columns, a set-point near 180 on the higher orders for
  weeping forms, and a set-point that rises along the limbs for the flat top of the
  umbrella. The curve starts short at the base: the lowest positions only exist on young
  trees, whose first branches are small.
- **Foliage** is carried by twigs (`[foliage.twig]`), several leaves on a stem, placed
  at two or three positions around each node of the last few growth units of every
  axis. The graph stops at fine branches; the twigs stand for the leafy shoots. Conifer
  twigs are whole branchlets or needle-clad shoots with fins for their bulk.
- **Trunks** use the pipe model where the crown divides into limbs (maple, oak, willow,
  acacia) and the tapered model where one trunk runs to the top.
- **Fronds** of the palm are branches with a `branch_lifespan` of a few cycles whose
  insertion angle grows with their age (`aging_angle`); their leaflets are the leaves.
  The fronds of the fern and the leaves of the rosette are stems rising from the ground.
- **Bark** has a `young_color` for thin axes: dark twigs on the white birch, orange
  limbs on the pine, green stalks on the palm.

## Age

A plant keeps growing until every axis has reached its full-grown length, which for the
trees takes longer than the oldest age of their range; beyond that the plant no longer
changes. Each preset states the range of ages it is meant for in `[variation]`:

| Preset | Ages | Preset | Ages |
|---|---|---|---|
| `acacia_like` | 20 to 45 | `oak_like` | 25 to 70 |
| `bamboo_like` | 3 to 10 | `palm_like` | 6 to 30 |
| `birch_like` | 10 to 45 | `pine_like` | 14 to 60 |
| `broadleaf` | 14 to 50 | `poplar_like` | 10 to 40 |
| `conifer` | 10 to 50 | `rosette_herb` | 1 to 2 |
| `cypress_like` | 10 to 40 | `sapling` | 3 to 8 |
| `fern_like` | 2 to 4 | `shrub` | 4 to 14 |
| `larch_like` | 15 to 50 | `spreading_shrub` | 4 to 12 |
| `maple_like` | 15 to 50 | `spruce_like` | 12 to 50 |
| | | `upright_shrub` | 4 to 16 |
| | | `willow_like` | 20 to 50 |

Below its range a tree is a thin sapling with little foliage, which is why the ranges of
the spreading trees start late. Node counts need not grow with age: a `bare_base` and a
`branch_lifespan` remove the lowest branches as the trunk lengthens, as self-pruning
does.

## Variation

The `[variation]` table of a species says how individuals differ ([Core](core.md)):

```toml
[variation]
age = [25, 70]          # ages worth generating, in growth cycles
scale = [0.85, 1.2]     # overall scale factors
weight = 1.0            # relative frequency in a random mix

[variation.jitter]      # relative half-width of a random factor per parameter
"growth.axis_length" = 0.15
"growth.branching_angle" = 0.12
"foliage.color" = 0.1
```

`Species::varied(seed, index)` returns an individual: each jittered parameter is
multiplied by a factor drawn uniformly within its half-width and clamped to its domain,
with one factor per branching order for curves and one per channel for colours. The
presets jitter lengths, angles, tropism, crookedness, trunk girth, leaf size and the
colours of leaves and bark.

`arbrume gen --random` and `arbrume_gen::RandomSampler` draw a preset by weight, an age
and a scale in its ranges and an individual, all from `(seed, index)`
([Command line](cli.md)).

## Triangle budgets

The foliage budget of a preset is set by three numbers: `foliage.per_node` and
`foliage.density` (twigs per node) and `foliage.twig.leaves`. A broadleaf tree of the
library carries 3 000 to 6 000 twigs of 5 to 8 leaves at the top of its age range, a
conifer 3 000 to 5 000 branchlets, each with 12 to 20 needles and two or three fins. At
full quality that is 100 000 to 300 000 triangles; `MeshOptions::with_quality` and the
budget of the random sampler bring a plant to a chosen size by keeping a fraction of the
twigs, enlarged to preserve the leaf area.

## Adding a preset

Drop `<name>.toml` in `crates/arbrume-gen/species/`, add its line to `PRESETS` in
`crates/arbrume-gen/src/presets.rs` (kept sorted) and its row to the table above. The
tests then grow it at three ages of its range, mesh it, export it and check that its
jitter gives valid individuals. Look at it before committing:

```sh
arbrume gen --species <name> --count 6 --age-range 10..40 --thumb 384 --out look
```
