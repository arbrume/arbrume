# Roots

A species with a `[roots]` table grows a root system below its collar, in the same plant
graph and by the same growth cycle as its shoot. A species without the table has no roots
and grows exactly as before. This page describes how roots are grown, the soil they grow
in, the parameters of `[roots]`, how they are meshed and labelled, and what the root model
leaves out.

```rust
use arbrume_core::species::{RootHabit, Roots};
use arbrume_grow::{GrowParams, Soil, Surroundings, grow_rooted};

let mut species = arbrume_gen::preset("maple_like").unwrap();
species.roots = Some(Roots::of(RootHabit::Heart));
let params = GrowParams { seed: 1, age: 30, scale: 1.0 };
let graph = grow_rooted(&species, &params, &Surroundings::none(), &Soil::homogeneous());
```

`arbrume_gen::generate` and every front end grow the roots of a species that has them; the
soil is `GenParams::soil` (`GenParams::with_soil`), homogeneous under flat ground by
default.

## How roots are grown

`arbrume_grow::add_roots` adds a root system to a grown shoot in five steps.

1. **Collar.** The plant is lowered so that the first node of its stem, the *collar*, is
   on the ground (`y = 0`). The first internode of the stem goes under the ground; its
   base is moved up to 1.5 radii under the collar once the radii are known (step 5).
2. **Growth.** The roots are grown by the growth cycle of the shoots (`run_cycle`) with a
   species built from `[roots]`: each field of `[roots]` becomes the field of `[growth]`
   of the same name, the axes are monopodial and grow continuously, and nothing bears
   leaves. The cycle runs in the frame of the plant *turned upside down* about the
   highest point of the ground: there the roots rise into the soil as shoots rise into the
   air, a set-point counted from the downward vertical is the set-point of the shoots,
   and the air is where the turned plant cannot dig, so the stages of the shoots apply as
   they are. The primary roots leave the collar spread around it, at the set-point of
   order 0 at their base.
3. **Radii.** The pipe radii of the root graph come from `tip_radius` at the root tips
   (`pipe_exponent`). A root graph has far fewer tips than a real root system, whose fine
   roots it leaves out, so its radii are too thin: with `pipe_balance = b` every radius of
   the roots of a stem is multiplied by `k^b`, `k` being the factor that makes the pipe
   sum of its primaries equal that of the shoot at the collar (the pipes of the shoot
   continue into the roots). The primaries are then widened near the collar by
   `1 + flare (1 - s / flare_length)^2`, `s` the arc length from the collar.
4. **Soil cover.** Every root node is moved down where needed so that the root is under
   the ground by its own radius. With `exposed > 0` the roots that run out near the
   surface are raised instead, close to the collar, so that the part `exposed` of their
   diameter shows above the ground, a weight fading with the square of the arc length
   over `exposed_length`; only roots whose path from the collar is closer to horizontal
   than to vertical are raised.
5. **Graft.** The root graph is copied into the graph of the shoot, every node flagged
   `NodeFlags::ROOT`. A primary root becomes a lateral axis of the collar, so the mesher
   joins it to the stem as it joins any branch. The first internode of the stem carries
   the pipes of both systems: its radius becomes `(r_shoot^n + sum of r_primary^n)^(1/n)`
   (`n` the `pipe_exponent` of the roots). Two kinds of root are treated apart:
   - a primary that leaves the collar so steeply that it would come out through the
     base of the stem rather than its side starts an axis of its own at the collar: it
     continues the stem downwards (a tap root);
   - a lateral root that would start inside the wood of the stem (within the flared
     radius of the base of the stem, above its foot) or on the first node of its parent
     (inside the junction of that parent) is shed: it stays in the graph and in the pipe
     sums, and is not meshed.

The root system depends only on the species, the parameters, the surroundings, the soil
and the shoot. Its random draws are keyed by meristem keys derived from the seed of the
plant and a salt, so roots never change the draws of the shoot; the only parallel pass is
the association of resource points to root tips, which gives the same result on any
number of threads. The tests check one and four threads bit for bit.

### Parametric and self-organizing roots

`roots.mode` chooses the stages, independently of the mode of the shoot.

- **Parametric** (default): the fate of the shoots (`GradientFate`) with the curves of
  `[roots]`. The soil sensing stage gives every root tip `Q = 1` and, as preferred
  direction `V`, the direction in which the richness of the soil rises (zero in
  homogeneous soil), weighted by `environment_weight`.
- **Self-organizing**: root tips compete for the resources of the soil, in the spirit of
  Rhizomorph (Li et al. 2023). The resource is a set of points scattered in the soil,
  denser in rich patches. Each cycle the points within `occupancy_radius` of a tip are
  taken up and removed, every other point in the perception cone of one or more tips goes
  to the closest ([`Markers`](growth.md#space-markers)); a tip that owns points has an
  uptake `Q` equal to the richness of the soil where it stands and prefers the direction
  of its points, a tip that owns none has `Q = 0`. The extended Borchert-Honda
  allocation of the shoots (`ResourceFate`, `axis_bias`, `resource_coefficient`) then
  shares the resource along each primary root, so roots in rich, unexplored soil grow at
  the expense of the others, and roots stop where they have depleted the soil. The
  curves of `[roots]` act as limits, as in the self-organizing mode of the shoots.

### Orientation, sinkers and obstacles

Each root internode takes the tropism blend of the shoots, `H' = alpha H + beta T +
gamma E`, with the curvature, gnarl and wander of `[roots]` (see
[Growth](growth.md#parametric-mode)). Two rules are particular to roots:

- **Tap root and sinkers.** With `tap_root`, the first primary leaves the collar straight
  down and its tropism points straight down. A lateral root becomes a *sinker* with the
  `sinker_probability`, drawn once from the key of its meristem: its tropism points
  straight down whatever its order. Li et al. (2023, eq. 23) switch between lateral and
  vertical tropism at every node with a probability falling with the distance to the
  base; here the draw is made once per root.
- **Obstacles and the ground.** A root does not stop at an obstacle or at the ground
  surface. When an internode would end in the air or in an obstacle it slides along the
  surface it meets (its heading less the part along the direction that leads out of the
  blocked place, from central differences of the distance to the nearest blocked place),
  then tries straight ahead, then headings turned away from the blocked place by 45, 90
  and 135 degrees at six azimuths; the root ends only when every one is blocked. In the
  self-organizing mode a tip whose next internode is blocked is also left out of the
  allocation, as a shoot is, so its share goes to the others.

## The soil

`arbrume_grow::Soil` (`arbrume_gen::Soil`) holds what only the roots see; obstacles are the
blocking occluders of the [surroundings](growth.md#surroundings), which block roots as
they block shoots: a rock is an occluder below the ground, a wall one that reaches into
it. Everything is in meters in the frame of the plant; the scale of the plant does not
apply.

| Field | Default | Meaning |
|---|---|---|
| `ground` | `flat` | the surface roots stay under: `flat` (`y = 0`), `slope` (`rise = [x, z]`, the plane through the origin of height `rise[0] x + rise[1] z`) or `heights` (an uneven terrain: `origin = [x, z]`, `cell`, `size = [nx, nz]`, `heights` X fastest, read by bilinear interpolation and continued flat beyond the border) |
| `resources` | none | patches richer than the homogeneous soil: a shape (as for an occluder), `richness` (added to the richness 1 of the soil inside it) and `reach` (meters outside the shape over which the richness fades as the square of the distance, the reach of the gradient that attracts roots) |

```toml
ground = { kind = "slope", rise = [0.15, 0.0] }

[[resources]]
shape = "sphere"
center = [3.0, -1.2, 0.0]
radius = 0.9
richness = 6.0
reach = 2.5
```

The roots grow in the turned frame, where the stages read obstacles as a `Surroundings`:
spheres, ellipsoids, boxes and cylinders are turned exactly; a cone, which has no
apex-down form, is voxelized, and a voxel mask is turned voxel by voxel. An uneven ground
adds the air above it as one more obstacle, a `VoxelMask` of voxels of one root internode
unit over the reach of the roots. The orientation stage reads the exact ground height.

The resource points of the self-organizing mode are scattered in a box around the collar
as wide and as deep as a primary root and a lateral end to end: `marker_density` per cubic
internode unit of homogeneous soil, plus `richness` times as many again in each patch
(fewer where it fades), all scaled down together to at most `marker_limit`.

## Parameters of `[roots]`

`habit` chooses the starting value of every other field, so a file lists only what
differs; `[roots]` alone gives the heart habit. Set-points are counted from the
*downward* vertical: 0 is straight down, 90 horizontal. Lengths are in meters at scale 1.
Every value is a starting point chosen by eye, not a measurement.

| Habit | Form | Main values |
|---|---|---|
| `tap` | a vertical tap root among long, near-horizontal primaries | 6 primaries, `tap_root`, set-point 78 to 55 |
| `heart` | oblique primaries that steepen with depth, a rounded mass | 7 primaries, set-point 64 to 35 |
| `plate` | long shallow primaries with sinkers | 9 primaries 6.5 m long, set-point 86, plagiotropic, sinkers 0.22 |
| `fibrous` | a dense tuft of thin primaries of similar length | 24 primaries 1.6 m long, `max_order` 1, set-point 70 to 15 |

| Field | Heart value | Meaning |
|---|---|---|
| `habit` | `heart` | `tap`, `heart`, `plate` or `fibrous` |
| `mode` | `parametric` | `parametric` or `self_organizing` (above) |
| `primaries` | 7 | primary roots leaving the collar (1 to 64), spread evenly around it |
| `max_order` | 2 | highest order, 0 for primaries alone (at most 4) |
| `axis_length` | 5, 1.8, 0.45 | length of a full-grown root, curve per order along the parent |
| `shoot_length` | 0.3, 0.2, 0.12 | extension per cycle, curve per order along the axis |
| `internode_length` | 0.15, 0.1, 0.06 | internode length, curve per order along the axis |
| `length_variation` | 0.15, 0.3, 0.3 | half-width of the random variation of `axis_length` |
| `branch_probability` | 0.6, 0.45, 0 | probability that a node bears a lateral, per order of the bearing root |
| `branching_angle` | 60, 70, 65 | angle between a lateral and its parent |
| `gravitropic_setpoint` | 64 to 35, 70, 55 | tropism angle from the downward vertical, along the axis |
| `tropism_weight` | 0.15, 0.1, 0.05 | weight `beta` of the tropism per internode |
| `direction_variation` | 9, 13, 18 | random deviation per internode, degrees |
| `gnarl` | 22, 25, 25 | random bend per cycle, degrees |
| `tap_root` | false | the first primary is a tap root (above) |
| `plagiotropic` | false | primaries keep their laterals on their two horizontal sides |
| `sinker_probability` | 0 | probability that a lateral root is a sinker |
| `environment_weight` | 0.3 | weight `gamma` of the direction preferred by the soil |
| `pipe_exponent` | 2 | `n` of the pipe model of the roots and of the collar |
| `tip_radius` | 0.005 m | radius of a root tip |
| `pipe_balance` | 1 | `b` of the pipe balance at the collar, in `[0, 1]` |
| `flare` | 0.6 | widening of a primary near the collar, as a fraction of its radius |
| `flare_length` | 1.2 m | length over which that widening fades |
| `exposed` | 0 | part of the diameter of the primaries above the ground at the collar, in `[0, 1]` |
| `exposed_length` | 1.5 m | length over which the exposure fades |
| `self_organization` | | `resource_coefficient` 2, `axis_bias` 0.5, `occupancy_radius` 1.5, `perception_distance` 5, `perception_angle_deg` 120, `marker_density` 0.15, `marker_limit` 200000 (lengths in internode units, the `internode_length` of order 1) |

## Meshing and labels

Roots are axes of the graph and are meshed like branches, with the bark and the
junctions of the species ([Meshing](mesh.md), [Junctions](junctions.md)): a primary root
is joined to its stem, and a lateral root to its parent, by a Tier A collar or a Tier B
stitched hole. The trunk flare of a stem with roots starts at its collar instead of its
base: it is widest at the ground and narrows over one eighth of the trunk above it, and
below the ground the base of the stem widens linearly from its foot up to the collar, so
the root flares merge into the flare of the trunk. A root axis that starts a chain of its
own (a tap root) has no flare.

Root vertices carry the semantic class `root` (value 4, appended after `trunk`, `branch`,
`twig` and `leaf`; [ML annotations](annotations.md)) and the chain id as instance. The
baked wind data of a root vertex is that of the stem where the roots leave it, so roots
stay still.

Growth sequences (`Generator::frame`, `--growth-frames`) grow the root system of each
frame for the age of that frame, under the shoot of that frame; the last frame is the
plant of `generate`. Plants of a stand grow their roots after the stand, each in its own
frame and soil, without the obstacles of the stand.

## Command line, Python and Blender

```sh
arbrume gen --species maple_like_rooted --age 40 --out rooted
arbrume gen --species broadleaf --roots plate --soil soil.toml --surroundings rock.toml --out plate
arbrume gen --species oak_like_rooted --roots none --out shoot_only
```

`--roots` takes a habit (which replaces the `[roots]` of the species), `species` (the
default) or `none`; `--soil` a soil file ([Command line](cli.md)). In Python,
`roots=` takes a bool, a habit name or a dict and `soil=` a dict or a TOML path
([Python](python.md)). In Blender the *Roots* panel chooses the roots of the plant and
edits the parameters of `[roots]` ([Blender](blender.md)).

The presets `oak_like_rooted` (tap), `maple_like_rooted` (heart), `spruce_like_rooted`
(plate, exposed) and `palm_like_rooted` (fibrous) are their base presets with a `[roots]`
table and a weight of 0, so that random mixes do not draw them.

## Cost

Generation (grow and mesh, full quality, seed 1, median of 7, milliseconds) on an AMD
Ryzen 7 9700F under Windows 11, measured on a loaded machine, to compare and not as
targets:

| preset | age | nodes | root nodes | ms | triangles |
|---|---|---|---|---|---|
| `oak_like` | 60 | 5 704 | 0 | 6.9 | 309 492 |
| `oak_like_rooted` | 60 | 15 277 | 9 573 | 10.7 | 447 075 |
| `maple_like` | 45 | 4 689 | 0 | 5.3 | 242 146 |
| `maple_like_rooted` | 45 | 15 701 | 11 012 | 10.4 | 401 243 |
| `spruce_like` | 45 | 2 442 | 0 | 4.0 | 167 725 |
| `spruce_like_rooted` | 45 | 11 847 | 9 405 | 7.6 | 279 699 |
| `palm_like` | 25 | 902 | 0 | 0.6 | 24 708 |
| `palm_like_rooted` | 25 | 2 253 | 1 351 | 1.2 | 48 004 |
| `maple_like_rooted`, self-organizing roots | 45 | 15 313 | 10 624 | 138 | 402 094 |

Parametric roots cost about as much per node as the shoot. Self-organizing roots are
dominated by the resource points: the box of a large root system holds the 200 000 points
of `marker_limit`, scattered and associated to the tips every cycle.

## What the model of Li et al. (2023) does and this one does not

The root system follows the spirit of Rhizomorph: root tips grow towards the resources of
the soil and compete for them, obstacles and the ground surface shape the system, and the
habits (tap, heart, plate, sinkers) are parameter sets of one model. Not modelled:

- the long-distance signals and the plant vigor `min(root flux, shoot flux)` shared
  between shoot and root by their maintenance (their eqs. 1 to 17): shoot and root grow
  each with its own resource, the root system after the shoot, and they are coupled by
  the pipe model at the collar only;
- the soil as a field of water and nutrients that diffuse and are carried by gravity
  (their eqs. 25 to 31; their eq. 31 as printed, `1 - exp(i / (0.2 w^2))`, leaves
  `[0, 1]` and needs `exp(-.)`): here the resource is static, homogeneous with richer
  patches, depleted only where root tips take it up;
- the growth resistance of the soil, which shortens internodes (their eqs. 5 and 22),
  and temperature;
- bud activation from signals (their eqs. 20 and 24): laterals form with the branching
  probability of the species;
- the per-node switch of tropism (their eq. 23): a root is drawn a sinker once;
- root shedding and turnover.

In return roots never enter an obstacle or the air, which Li et al. state their model
does not prevent.

## Limits

- Tier B junctions on rooted presets: on `maple_like_rooted`, `oak_like_rooted`,
  `palm_like_rooted` and `spruce_like_rooted` at ages 5, 12, 20, 30 and 45, seeds 1 to 5
  (100 meshes), 3 meshes have one non-manifold edge at a junction between two roots
  (`palm_like_rooted` 20 seed 3, 45 seeds 4 and 5), where a
  lateral leaves a bending root close to that root's own junction; the junction test
  ages 5 and 12 are closed manifolds.
- A tap root is not joined to its stem: it starts inside the base of the stem, a
  separate tube in Tier A and a separate closed surface in Tier B.
- The base of the stem under the collar is a short cone; in a cutaway it shows as a
  root crown with a flat foot.
- Exposed roots are raised toward the surface but keep a round section: buttresses are
  the widened bases of the primaries merging with the trunk flare, not planks.
- Billboard levels of detail render the whole mesh, roots included.
- Shed laterals near the collar stay in the skeleton annotations (`node_shed = 1`).

## References

- Barthélémy and Caraglio 2007, Plant architecture: a dynamic, multilevel and
  comprehensive approach to plant form, structure and ontogeny, Annals of Botany.
- Li, Klein, Michels, Benes, Pirk and Pałubicki 2023, Rhizomorph: the coordinated
  function of shoots and roots, ACM Transactions on Graphics.
- Palubicki et al. 2009, Self-organizing tree models for image synthesis, ACM TOG.
- Runions, Lane and Prusinkiewicz 2007, Modeling trees with a space colonization
  algorithm, Eurographics Workshop on Natural Phenomena.
- Shinozaki, Yoda, Hozumi and Kira 1964, A quantitative analysis of plant form: the pipe
  model theory, Japanese Journal of Ecology.
