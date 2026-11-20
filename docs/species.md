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
| `oak_like` | broadleaf tree | stout upright trunk dividing into heavy crooked limbs, dome wider than tall | 60 | 14 m | 18 m | 0.7 m | 350 k |
| `birch_like` | broadleaf tree | slender white trunk, thin rising branches, fine pendulous twigs, light crown | 30 | 15 m | 7 m | 0.25 m | 155 k |
| `poplar_like` | broadleaf tree | columnar: branches rise close to the trunk in a tall spire | 30 | 19 m | 4.5 m | 0.4 m | 145 k |
| `willow_like` | broadleaf tree | weeping: arching limbs, long branchlets hanging in curtains of narrow leaves | 30 | 12 m | 12.5 m | 0.35 m | 140 k |
| `acacia_like` | broadleaf tree | umbrella: bare trunk, limbs that rise then flatten into a wide flat top of feathery leaves | 30 | 11 m | 17 m | 0.35 m | 180 k |
| `sapling` | broadleaf tree | a young whip with a few short side branches and large leaves | 5 | 2.5 m | 1.6 m | 3 cm | 2 k |
| `conifer` | conifer | fir-like: regular whorls of level branches with flat sprays, tidy dense cone | 40 | 16 m | 6.5 m | 0.3 m | 105 k |
| `spruce_like` | conifer | whorls that sweep down and turn up at the tip, hanging branchlets, narrow cone | 40 | 18 m | 8.5 m | 0.35 m | 160 k |
| `pine_like` | conifer | cleared trunk, open whorls of upturned limbs, tufts of long needles, rounded top | 45 | 18 m | 12 m | 0.4 m | 260 k |
| `cypress_like` | conifer | columnar: short upright branches and dense dark sprays in a narrow flame | 30 | 12 m | 3 m | 0.25 m | 100 k |
| `larch_like` | conifer | level limbs, thin hanging branchlets in soft light-green tufts, open broad cone | 40 | 18 m | 9.5 m | 0.4 m | 200 k |
| `palm_like` | palm | unbranched trunk crowned by arching feather fronds, old ones drooping | 20 | 7 m | 5.5 m | 0.2 m | 26 k |
| `shrub` | shrub | rounded dense bush of many stems, small leaves | 8 | 2 m | 3 m | 4 cm | 80 k |
| `upright_shrub` | shrub | clump of tall canes rising steeply, broad leaves on short side shoots | 10 | 4.5 m | 3.5 m | 6 cm | 90 k |
| `spreading_shrub` | shrub | stems running out low over the ground, wider than tall, small dark leaves | 8 | 1.1 m | 4 m | 4 cm | 130 k |
| `bamboo_like` | cane | clump of thin green canes, bare below, whorls of twigs and narrow leaves above | 6 | 8 m | 4 m | 5 cm | 75 k |
| `fern_like` | herb | full shuttlecock of arching fronds with two ranks of feathered pinnae | 2 | 0.8 m | 1.7 m | | 50 k |
| `rosette_herb` | herb | low clump of large stalked leaves from one point, hosta-like | 1 | 0.45 m | 1 m | | 1.2 k |
| `open_grown_broadleaf` | broadleaf tree, self-organizing | full crown on every side that keeps its low limbs and thins out inside | 25 | 16 m | 16 m | 0.45 m | 330 k |
| `forest_grown_broadleaf` | broadleaf tree, self-organizing | tall clean bole under a high narrow crown; shaded limbs are shed | 40 | 27 m | 14 m | 0.65 m | 215 k |
| `excurrent_conifer` | conifer, self-organizing | one straight leader, whorls shorter towards the top, lowest whorls shed (priority model) | 30 | 17.5 m | 9 m | 0.25 m | 165 k |
| `spreading_decurrent` | broadleaf tree, self-organizing | short trunk dissolving into wide level limbs, low dome twice as wide as tall | 25 | 9 m | 20 m | 0.45 m | 150 k |
| `thicket_shrub` | shrub, self-organizing | several leaning stems, twigs crowded at the lit surface of a rounded bush | 10 | 1.7 m | 2.3 m | 2.5 cm | 28 k |
| `hedge_shrub` | shrub, self-organizing | dense small-leaved bush that fills and stops at a clip volume | 10 | 1.4 m | 2 m | 2.5 cm | 24 k |
| `envelope_tree` | broadleaf tree, self-organizing | crown that fills an envelope, by default an ellipsoid (markers and light) | 25 | 9.5 m | 8 m | 0.18 m | 130 k |
| `ivy_like` | climber, self-organizing, adhesive | climbs a wall and fans out over it, wraps over its top and around a window; creeps on open ground. Size on the wall of `examples/climbing/window_wall.toml` | 16 | 4.1 m | 5.8 m | 6 cm | 65 k |
| `vine_like` | climber, self-organizing, tendril | climbs a post with coiled tendrils, runs along the beams of a pergola; size on `examples/climbing/pergola.toml` | 10 | 2.9 m | 3 m | 3.5 cm | 18 k |
| `creeper_like` | ground cover, self-organizing, adhesive | stems running out level over the ground and over rocks, small leaves facing up; on open ground | 12 | 5 cm | 4.4 m | 2 cm | 100 k |
| `oak_like_rooted` | broadleaf tree with roots | `oak_like` over a tap root among long near-horizontal roots ([Roots](roots.md)) | 60 | 14 m | 18 m | 0.7 m | 445 k |
| `maple_like_rooted` | broadleaf tree with roots | `maple_like` over a heart root system: oblique roots in a rounded mass | 45 | 13 m | 12 m | 0.4 m | 400 k |
| `spruce_like_rooted` | conifer with roots | `spruce_like` over a plate root system with sinkers, the primaries showing at the trunk base | 45 | 18 m | 8.5 m | 0.35 m | 280 k |
| `palm_like_rooted` | palm with roots | `palm_like` over a dense tuft of thin fibrous roots | 25 | 7 m | 5.5 m | 0.2 m | 48 k |

`broadleaf`, `conifer` and `shrub` are the generic presets; the three files of the same
names in `crates/arbrume-grow/tests/data/` are older starter species kept as test
fixtures of the growth engine.

## How the presets are built

Ten presets grow in the self-organizing mode (the three climbers and the seven of the
next section); the others use the parametric growth mode ([Growth](growth.md)). All use
the foliage and bark of the mesher ([Meshing](mesh.md)). In the parametric presets a few devices recur:

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
- **Textures** ([Textures](textures.md)) come from `[bark].pattern` and the `venation`
  and `margin` of `[foliage]`: smooth bark with lenticels on the birch, sapling, bamboo,
  beech-like forest-grown broadleaf and the ringed palm (lenticels stretched into bands),
  furrowed bark on the oak, maple, poplar, willow, acacia and the generic broadleaves,
  plated bark on the pine, larch, spruce and conifers, fibrous bark on the cypress and the
  shrubs, peeling bark on the spreading decurrent tree. Leaves: lobed oak leaves,
  palmate lobed maple leaves, serrate birch, willow and broadleaf leaves, crenate poplar
  and beech leaves, parallel veins on the bamboo, palm and cypress, open venation on the
  spreading decurrent tree, and needles with two opaque variants and the spray of their
  fins on the conifers. Trees use 512 texel bark and leaves, shrubs and needles 256, herbs
  less.

## Climbing presets

`ivy_like`, `vine_like` and `creeper_like` have a `[climbing]` table
([Climbing plants](climbing.md)): their shoots hold the blocking volumes of the
surroundings and the ground. Their `variation.weight` is 0, so the random sampler never
draws them: a climber needs a support to be itself, and giving every random plant a wall
would change the random set. Generated without surroundings they creep as a ground cover.

## Rooted presets

`oak_like_rooted`, `maple_like_rooted`, `spruce_like_rooted` and `palm_like_rooted` are
their base presets with a `[roots]` table ([Roots](roots.md)) of the tap, heart, plate
and fibrous habits; the oak lengthens its roots for its size and the spruce shows the
upper side of its primaries near the trunk (`exposed = 0.4`). Their shoots are those of
the base presets, lowered so that the first node of the trunk is on the ground. Their
`variation.weight` is 0, so random mixes are unchanged. Heights and widths in the table
of presets are those of the shoots; triangles include the roots.

## Self-organizing presets

These presets set `growth.mode = "self_organizing"`: their buds compete for light, their
shaded branches are shed, and they respond to the surroundings given to the generator
(an envelope, obstacles, the shade of neighbours). Their habit comes from a few
parameters of `[growth.self_organization]` rather than from length curves:

| Preset | Sensing | Allocation | What sets the habit |
|---|---|---|---|
| `open_grown_broadleaf` | shadow | Borchert-Honda | `axis_bias` 0.52, a low `shedding_threshold` (0.02), no bud on the lowest tenth of the trunk |
| `forest_grown_broadleaf` | shadow | Borchert-Honda | `axis_bias` 0.55 on the trunk, `shedding_threshold` 0.05, narrow branching angles, no bud on the lowest tenth of the trunk |
| `excurrent_conifer` | shadow | priority | apical control on the trunk and the branches, whorls at the end of each growth unit, an `axis_length` of order 1 that shortens towards the top |
| `spreading_decurrent` | shadow | Borchert-Honda | `axis_bias` 0.42 on the trunk, a set-point near horizontal with a strong `tropism_weight`, a small `environment_weight` |
| `thicket_shrub` | shadow | Borchert-Honda | seven stems, `axis_bias` 0.47, a light shadow (`shadow_strength` 0.1 over 4 layers) |
| `hedge_shrub` | shadow | Borchert-Honda | five stems, decussate buds, an even lighter shadow so that buds stay alive inside a clipped volume |
| `envelope_tree` | both | Borchert-Honda | markers in the envelope, `environment_weight` 0.35 |

In these files `axis_length` and `shoot_length` are limits (where an axis stops, how long
one shoot can be), not what the plant is drawn from. The sizes in the table of presets
are those of plants on open ground; the same seed in other surroundings gives another
plant. The crowns of `hedge_shrub` and `envelope_tree` are the ones to give an envelope
to: in a tight envelope the species with a dense shadow thin out, because light only
comes from above and their lit surface is the part that is clipped.

Scenes to try (`--surroundings` takes the TOML file of the surroundings, see
[Command line](cli.md)):

```sh
# a tree next to a wall: the crown leans away and nothing enters the wall
arbrume sheet --species open_grown_broadleaf --ages 12,22 --azimuths 0,90 --surroundings wall.toml --out wall.png
# two trees sharing their light
arbrume sheet --species open_grown_broadleaf --ages 15,25 --row 2 --spacing 5 --out pair.png
# a crown in an envelope, and a hedge row clipped to a box
arbrume sheet --species envelope_tree --ages 10,18,28 --surroundings cone.toml --out cone.png
arbrume sheet --species hedge_shrub --ages 5,9,14 --row 5 --spacing 0.8 --surroundings box.toml --out hedge.png
```

## Age

A plant keeps growing until every axis has reached its full-grown length, which for the
trees takes longer than the oldest age of their range; beyond that the plant no longer
changes. Each preset states the range of ages it is meant for in `[variation]`:

| Preset | Ages | Preset | Ages |
|---|---|---|---|
| `acacia_like` | 24 to 45 | `oak_like` | 30 to 70 |
| `bamboo_like` | 3 to 10 | `palm_like` | 6 to 30 |
| `birch_like` | 10 to 45 | `pine_like` | 14 to 52 |
| `broadleaf` | 16 to 50 | `poplar_like` | 10 to 40 |
| `conifer` | 10 to 50 | `rosette_herb` | 1 to 2 |
| `cypress_like` | 10 to 40 | `sapling` | 3 to 8 |
| `fern_like` | 2 to 4 | `shrub` | 4 to 14 |
| `larch_like` | 15 to 50 | `spreading_shrub` | 4 to 12 |
| `maple_like` | 18 to 50 | `spruce_like` | 12 to 50 |
| | | `upright_shrub` | 5 to 16 |
| | | `willow_like` | 24 to 50 |
| `envelope_tree` | 12 to 22 | `open_grown_broadleaf` | 10 to 25 |
| `excurrent_conifer` | 12 to 34 | `spreading_decurrent` | 10 to 30 |
| `forest_grown_broadleaf` | 15 to 45 | `thicket_shrub` | 5 to 11 |
| `hedge_shrub` | 5 to 10 | `ivy_like` | 8 to 20 |
| `vine_like` | 6 to 14 | `creeper_like` | 6 to 14 |
| `oak_like_rooted` | 30 to 70 | `maple_like_rooted` | 18 to 50 |
| `spruce_like_rooted` | 12 to 50 | `palm_like_rooted` | 6 to 30 |

Below its range a tree is a thin sapling with little foliage, which is why the ranges of
the spreading trees start late. Node counts need not grow with age: a `bare_base` and a
`branch_lifespan` remove the lowest branches as the trunk lengthens, as self-pruning
does. A self-organizing preset has no full-grown state set in advance: it grows as long
as its buds find light below the limits of `axis_length`, and past its range of ages the
shrubs turn leggy and the trees very large.

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
colours of leaves and bark; the self-organizing ones also jitter `axis_bias`, the
`resource_coefficient` or the `shedding_threshold`, by a few percent, since the habit is
sensitive to them.

`arbrume gen --random` and `arbrume_gen::RandomSampler` draw a preset by weight, an age
and a scale in its ranges and an individual, all from `(seed, index)`
([Command line](cli.md)).

## Triangle budgets

The foliage budget of a preset is set by three numbers: `foliage.per_node` and
`foliage.density` (twigs per node) and `foliage.twig.leaves`. A broadleaf tree of the
library carries 3 000 to 6 000 twigs of 5 to 8 leaves at the top of its age range, a
conifer 3 000 to 5 000 branchlets, each with 8 to 12 needles and two to four fins. At
full quality that is 100 000 to 300 000 triangles; `MeshOptions::with_quality` and the
budget of the random sampler bring a plant to a chosen size by keeping a fraction of the
twigs, enlarged to preserve the leaf area.

## Adding a preset

To share species without changing Arbrume, put them in a species pack instead
([Species packs](packs.md)): `arbrume pack add-species` copies a preset or a species
file into a pack, whose species are then named `pack_id/name`.

Drop `<name>.toml` in `crates/arbrume-gen/species/`, add its line to `PRESETS` in
`crates/arbrume-gen/src/presets.rs` (kept sorted) and its row to the table above. The
tests then grow it at three ages of its range, mesh it, export it and check that its
jitter gives valid individuals. Look at it before committing:

```sh
arbrume gen --species <name> --count 6 --age-range 10..40 --thumb 384 --out look
```
