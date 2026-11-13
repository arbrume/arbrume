# Growth

`arbrume-grow` turns a `Species` and a seed into a `PlantGraph`. This page describes the
growth cycle, the parametric mode that ships today, every growth parameter of a species,
and what the result contains.

```rust
use arbrume_core::species::Species;
use arbrume_grow::{GrowParams, grow};

let species = Species::from_path("crates/arbrume-grow/tests/data/broadleaf.toml")?;
let graph = grow(&species, &GrowParams { seed: 42, age: 25, scale: 1.0 });
```

| Parameter | Meaning |
|---|---|
| `seed` | seed of every random draw |
| `age` | number of growth cycles; 0 gives an empty graph |
| `scale` | factor on every length and radius of the graph; leaf site scales are not affected |

The returned graph satisfies `check_invariants()`, is contiguous, has its radii set and
carries leaf sites. The species must be valid, which species loaded from TOML are.

## The growth cycle

A plant grows by repeating one cycle. In each cycle every growing axis extends once, so
`age` counts the shoots of the oldest axis. With rhythmic growth one cycle makes one
growth unit, which for most temperate trees is one year.

The cycle has pluggable stages, the structure shared by Palubicki et al. (2009) and
Longay et al. (2012). Each stage is a trait in `arbrume_grow::stage`.

| Stage | Trait | Role | Available now |
|---|---|---|---|
| 1 | `Environment` | gives each bud an exposure `Q` and a preferred direction `V` | `NoEnvironment` (`Q = 1`, no `V`) |
| 2 | `BudFate` | decides which buds form, which flush, and how long each shoot is | `GradientFate` |
| 3 | `ShootOrientation` | direction of each new metamer | `TropismBlend`, `Straight` |
| 4 | `Shedding` | removes branches | `NoShedding` |

Between stages 2 and 3 the *shoot builder* does what no growth mode changes: it appends
the metamers, places leaves and axillary buds around each node from the phyllotaxis,
gives each bud its insertion angle, and ends the apex (last shoot, relay of a sympodial
axis, or fork). After the last cycle, `finish` makes chains contiguous, keeps the leaf
sites that are retained, computes the radii and applies bending under weight.

`grow` runs `NoEnvironment`, `GradientFate`, `TropismBlend` and `NoShedding`: the
parametric mode. `grow_with` runs any combination of stages; self-organizing stages
(shadow propagation, space colonization, resource allocation between buds, shedding) are
meant to be added as further implementations of the same traits.

### Meristems

The engine keeps one *meristem* per axis next to the graph (`Plant::meristems`): its
state (resting, growing, relayed, done), heading, a transported side vector, the arc
length built so far and the length of the full-grown axis. A meristem is created when its
bud forms and builds one chain.

### Determinism

The result depends only on the species, the parameters, the seed and the crate version.
One plant is grown sequentially; plants can be grown in parallel since nothing is shared
between calls. Every meristem has a 64-bit key derived from the seed and from its place
in the plant: the stem number, then for each ancestor the index of the bearing metamer
and the slot of the bud. Every random draw is seeded by such a key, with a stream and a
counter derived from the index of the metamer, never by a node id (node ids change when
the graph is compacted) nor by an order of creation.

### Age

Except for one parameter, the stages of the parametric mode do not look at `age`. A plant
grown for `n` cycles is then exactly the nodes born before cycle `n` of the same plant
grown longer: same chains, same positions. The exception is `bare_base`, which removes
the lowest laterals as the bearing axis gets longer; everything else stays in place,
because the random draws of an axis do not depend on which other axes exist. Radii, leaf
sites and bending are computed at the end and do depend on the age.

## Parametric mode

Following Prusinkiewicz et al. (2001), the parameters are curves over the branching order
and a relative position in `[0, 1]`, either *along the parent* (where an axis is
attached) or *along the axis* (where a metamer lies on its own axis). Relative positions
refer to the full-grown length of the axis, which is fixed when its bud forms.

Bud formation. On each new node the buds follow the leaves: `leaves_per_node` positions,
turned by the divergence angle from node to node. A bud forms with probability
`branch_probability`. No bud forms above `max_order`, on a short axis, or on the
`bare_base` of its bearing axis. A bud whose axis would be shorter than half an internode
aborts.

Axis length. An admitted bud gets the full-grown length of its axis at once:

$$
L = \mathit{axis\_length}(o, x)\,(1 - d\,x)\,
\left(\frac{L_{parent}}{L_{ref}}\right)^{e} (1 + v\,u)
$$

with $o$ the order of the axis, $x$ its attachment position along the parent, $d$ the
`apical_dominance`, $e$ the `parent_length_exponent`, $L_{ref}$ the largest
`axis_length` of the parent's order, $v$ the `length_variation` and $u$ uniform in
$[-1, 1)$. The curve of order 1 over $x$ draws the silhouette of the crown.

Extension. Each cycle the potential length of a growing axis increases by `shoot_length`
until it reaches $L$. Metamers of `internode_length` are added while they fit under the
potential, which integrates the node density of Prusinkiewicz et al. (2001, eq. 11); the
metamers of one shoot share its length equally. The last shoot is divided into the
nearest whole number of internodes, so an axis ends on $L$ to within half an internode.

Timing. A lateral bud flushes the cycle after it formed (`proleptic`) or in the same
cycle (`sylleptic`).

Orientation. Each metamer takes the heading (Longay et al. 2012, eq. 2)

$$
H' = \alpha H + \beta T + \gamma E, \qquad \alpha + \beta + \gamma = 1
$$

where $T$ makes the angle `gravitropic_setpoint` with the upward vertical in the vertical
plane of $H$, $\beta$ is `tropism_weight`, $E$ is the direction preferred by the
environment and $\gamma$ is `environment_weight` (0 without an environment). The heading
is then turned once by the sum of three small rotations: `curvature`, `gnarl` and
`direction_variation`.

## Architecture traits used by growth

The `[architecture]` table selects behaviours; the terms are those of Barthélémy and
Caraglio (2007).

| Field | Effect on growth |
|---|---|
| `branching` | `monopodial`: one apex builds the axis, and may fork. `sympodial`: the apex dies at the end of each shoot and a relay axis of the same order continues from the first leaf position of the last node, turned by `relay_angle` |
| `growth_rhythm` | `rhythmic`: the first metamer of each shoot is flagged `GROWTH_UNIT_START`, and tiers and whorls are available. `continuous`: no flag, no tier, no whorl |
| `branching_pattern` | `diffuse`: each bud forms with `branch_probability`. `continuous`: every bud with a positive probability forms. `rhythmic`: the probability is multiplied by `unit_branching` inside each growth unit (needs rhythmic growth, otherwise it behaves as diffuse) |
| `branch_timing` | `proleptic`: one cycle of rest before a lateral flushes. `sylleptic`: none |
| `max_order` | highest branching order produced |
| `phyllotaxis` | `leaves_per_node` and `divergence_deg`: 1 and 137.5 spiral, 1 and 180 distichous, 2 and 90 decussate, more than 2 whorled |
| `axes[order].orientation` | `orthotropic`: leaves and buds all around the axis. `plagiotropic`: leaves and buds on the two horizontal sides of the axis, leaf normals up |
| `axes[order].short` | a short axis bears no lateral |
| `axes[order].phyllotaxis` | phyllotaxis of that order when it differs |

## Growth parameters

All are fields of the `[growth]` table. "Curve" means a curve per order over the stated
position; a bare number is a constant. Angles are in degrees, lengths in meters. The
defaults are starting points for tuning, not measurements.

### Lengths and extension

| Field | Type | Position | Meaning |
|---|---|---|---|
| `axis_length` | curve | along the parent | length of a full-grown axis; order 0 is the height of the trunk |
| `shoot_length` | curve | along the axis | extension per cycle; with `axis_length` it sets how many cycles an axis takes to mature |
| `internode_length` | curve | along the axis | internode length, the inverse of node density |
| `length_variation` | curve | along the parent | half-width of the uniform random variation of `axis_length`, as a fraction |
| `apical_dominance` | number in `[0, 1]` | | shortens laterals towards the tip of their parent by the factor `1 - apical_dominance * x`; 1 gives a conical crown |
| `parent_length_exponent` | number | | exponent `e` above: 0 makes `axis_length` absolute, 1 makes laterals proportional to their parent |

### Branching

| Field | Type | Position | Meaning |
|---|---|---|---|
| `branch_probability` | curve, order of the bearing axis | along the bearing axis | probability that a bud forms |
| `bare_base` | curve, order of the bearing axis | attachment of the bearing axis along its own parent | fraction of the axis from its base without laterals, counted on the length the axis has at the age of the plant |
| `unit_branching` | single curve | position of the node in its growth unit | factor on `branch_probability` for a rhythmic branching pattern; rising towards 1 puts branches in tiers at the end of each unit |
| `whorl` | curve, order of the bearing axis | along the axis | number of buds spread evenly on the last node of each growth unit; below 0.5 the node follows the phyllotaxis |
| `branching_angle` | curve, order of the lateral | along the parent | insertion angle between a lateral and its parent |
| `branching_angle_variation` | curve | along the parent | half-width of its uniform random variation |
| `phyllotaxis_variation` | number | | half-width of the random variation of the divergence angle |

### Shape of an axis

| Field | Type | Position | Meaning |
|---|---|---|---|
| `gravitropic_setpoint` | curve | along the axis | angle of the tropism direction from the upward vertical: 0 up, 90 horizontal, 180 down |
| `tropism_weight` | curve | along the axis | weight `beta` of the tropism direction, per metamer |
| `environment_weight` | number | | weight `gamma` of the direction preferred by the environment |
| `curvature` | curve | along the axis | systematic bend counted over the whole axis: a metamer of length `l` on an axis of length `L` turns by `curvature * l / L`, downwards when positive; a sign change gives an S shape |
| `gnarl` | curve | along the axis | half-width of a random bend drawn once per cycle and spread over the metamers of that cycle |
| `direction_variation` | curve | along the axis | half-width of a random deviation drawn at each metamer |

### Forks, relays and stems

| Field | Type | Position | Meaning |
|---|---|---|---|
| `fork_probability` | curve | along the axis | probability per cycle that a monopodial axis forks at the end of its shoot; both axes keep the order and the full-grown length |
| `fork_angle` | curve | along the axis | angle by which each axis of a fork turns away; the fork opens by twice this |
| `relay_angle` | curve | along the axis | angle between a relay of a sympodial axis and the axis it continues |
| `stems` | integer | | number of stems rising from the ground |
| `stem_spread` | number | | radius of the circle on which several stems start |
| `stem_angle` | number | | outward lean of each of several stems; its variation is `branching_angle_variation` of order 0 |

### Bending under weight

`weight_bending` is the compliance $c = \rho g / E$ of the wood in 1/m (specific weight
over Young's modulus); 0 disables the pass. Each internode is a cantilever in the
small-deflection approximation: with $V$ the volume of wood it carries, $d$ the horizontal
distance from its base to the centre of mass of that wood, $l$ its length and $r$ its
radius, the internode and all it carries turn downwards by

$$
\theta = c\,\frac{V\,d\,l}{\pi r^4 / 4}
$$

limited to the angle that brings the centre of mass below the base. The pass runs once
from base to tips. It is a first-order estimate: it does not iterate to equilibrium,
leaves add no load, and the reaction wood by which real axes resist sagging is ignored.

`[growth.self_organization]` is reserved for the self-organizing stages and is not read
by the parametric mode.

## Radii

The `[geometry]` table chooses the model.

- `pipe` (Shinozaki et al. 1964): $r^n = \sum r_{child}^n$ from `tip_radius` at the tips,
  with $n$ the `pipe_exponent`.
- `tapered` (Weber and Penn 1995): the base radius of a stem is `radius_ratio` times its
  length; the base radius of a lateral is that of its parent times
  $(L / L_{parent})^{p}$ with $p$ the `child_radius_exponent`, and at most the radius of
  the parent at the attachment; along the axis the radius is the base radius times
  `taper`. Lengths are those at the age of the plant. A relay continues the arc length and
  keeps the base radius of the axis it relays.

`flare` is not applied to the radii; it is a profile for the mesher.

## Leaf sites

Each leaf is an `OrganSite` of kind `Leaf` on the node that bears it.

| Field | Value |
|---|---|
| `direction` | unit vector from the stem along the leaf, at `foliage.inclination_deg` from the axis |
| `normal` | unit vector orthogonal to `direction`: the upper side of the blade. It lies in the plane of the axis and the leaf and faces the tip of the axis; on a plagiotropic axis it faces up |
| `scale` | `foliage.size_along_axis` at the position of the node along its axis |
| `birth` | growth cycle of the node |

A node has `leaves_per_node` sites, at the same azimuths as its buds. With `r` the
`foliage.retention_cycles`, a node keeps its leaves when it was born in the last `r`
cycles, or when it belongs to the last `r` shoots of an axis that has finished growing:
the tip of a full-grown axis stays leafy, as if it kept making short shoots. For a
conifer, one site stands for the needles of one node.

## Starter species

`crates/arbrume-grow/tests/data/` holds three species files that exercise the mode: a
broadleaf tree (`broadleaf.toml`: diffuse branching, plagiotropic branches, forks, pipe
radii, bending), a conifer (`conifer.toml`: whorls at the end of each growth unit,
conical crown, tapered radii, four cycles of needles) and a shrub (`shrub.toml`: six
stems, sympodial axes, immediate branching). They are illustrations to tune from.

`cargo bench -p arbrume-grow` prints the node, chain and leaf site counts of each at
three ages and measures the growth time.

## Reference fixtures

The tree models of Honda and of Aono and Kunii, as given in chapter 2 of Prusinkiewicz and
Lindenmayer (1990), are regression tests (`tests/abop.rs`). They run through the same
cycle with a custom `BudFate` for the contraction ratios, and check segment counts per
step and per order, contraction ratios, branching angles, divergence, branch planes and
total length against values derived from the published productions and tables.

## References

- Barthélémy and Caraglio 2007, Plant architecture: a dynamic, multilevel and
  comprehensive approach to plant form, structure and ontogeny, Annals of Botany.
- Longay, Runions, Boudon and Prusinkiewicz 2012, TreeSketch, Eurographics SBIM.
- Palubicki et al. 2009, Self-organizing tree models for image synthesis, ACM TOG.
- Prusinkiewicz and Lindenmayer 1990, The Algorithmic Beauty of Plants, Springer.
- Prusinkiewicz, Mündermann, Karwowski and Lane 2001, The use of positional information
  in the modeling of plants, SIGGRAPH.
- de Reffye et al. 1988, Plant models faithful to botanical structure and development,
  SIGGRAPH.
- Shinozaki, Yoda, Hozumi and Kira 1964, A quantitative analysis of plant form: the pipe
  model theory, Japanese Journal of Ecology.
- Weber and Penn 1995, Creation and rendering of realistic trees, SIGGRAPH.
