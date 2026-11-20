# Growth

`arbrume-grow` turns a `Species` and a seed into a `PlantGraph`. This page describes the
growth cycle, its two modes (parametric and self-organizing), every growth parameter of a
species, the surroundings a plant can respond to, and what the result contains.

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

| Stage | Trait | Role | Parametric | Self-organizing |
|---|---|---|---|---|
| 1 | `Environment` | gives each bud an exposure `Q` and a preferred direction `V` | `NoEnvironment` (`Q = 1`, no `V`) | `Competition` |
| 2 | `BudFate` | decides which buds form, which flush, and how long each shoot is | `GradientFate` | `ResourceFate` |
| 3 | `ShootOrientation` | direction of each new metamer; may end a shoot early | `TropismBlend` | `Confined<TropismBlend>` |
| 4 | `Shedding` | removes branches | `NoShedding` | `ExposureShedding` |

Between stages 2 and 3 the *shoot builder* does what no growth mode changes: it appends
the metamers, places leaves and axillary buds around each node from the phyllotaxis,
gives each bud its insertion angle, and ends the apex (last shoot, relay of a sympodial
axis, or fork). After the last cycle, `finish` makes chains contiguous, keeps the leaf
sites that are retained, computes the radii and applies bending under weight.

A species selects its mode with `growth.mode`, `"parametric"` (the default) or
`"self_organizing"`; both read the same species file. `grow` runs the stages of that
mode, `grow_with` any combination of stages, and `run_cycle` one cycle of a plant, for
callers that drive the loop themselves.

### Meristems

The engine keeps one *meristem* per axis next to the graph (`Plant::meristems`): its
state (resting, growing, relayed, done), heading, a transported side vector, the arc
length built so far and the length of the full-grown axis. A meristem is created when its
bud forms and builds one chain.

### Determinism

The result depends only on the species, the parameters (surroundings included), the
seed and the crate version, never on the number of threads. The structure of a plant is
built on one thread; in the self-organizing mode the sensing of buds and markers may use
the `rayon` pool, each result being written to its own slot and the sums made
afterwards in a fixed order (see [Performance and engine
options](#performance-and-engine-options)). Plants can be grown in parallel since
nothing is shared between calls. Every meristem has a 64-bit key derived from the seed and from its place
in the plant: the stem number, then for each ancestor the index of the bearing metamer
and the slot of the bud. Every random draw is seeded by such a key, with a stream and a
counter derived from the index of the metamer, never by a node id (node ids change when
the graph is compacted) nor by an order of creation.

### Age

Except for two parameters, the stages of the parametric mode do not look at `age`. A
plant grown for `n` cycles is then exactly the nodes born before cycle `n` of the same
plant grown longer: same chains, same positions. The exceptions are `bare_base`, which
removes the lowest laterals as the bearing axis gets longer, and `branch_lifespan`, which
removes laterals older than a number of cycles (and `aging_angle`, which turns them as
they age); everything else stays in place, because the random draws of an axis do not
depend on which other axes exist. Radii, leaf sites and bending are computed at the end
and do depend on the age.

A plant stops changing once every axis has reached its full-grown length. How long that
takes follows from `axis_length` and `shoot_length`: a `shoot_length` curve that falls
along the trunk gives fast early growth that slows towards the full height, and a trunk
that needs more cycles than the ages one generates never saturates. Node counts need not
increase with age, since `bare_base` and `branch_lifespan` shed branches.

## Parametric mode

Following Prusinkiewicz et al. (2001), the parameters are curves over the branching order
and a relative position in `[0, 1]`, either *along the parent* (where an axis is
attached) or *along the axis* (where a metamer lies on its own axis). Relative positions
refer to the full-grown length of the axis, which is fixed when its bud forms.

Bud formation. On each new node the buds follow the leaves: `leaves_per_node` positions,
turned by the divergence angle from node to node. A bud forms with probability
`branch_probability`. No bud forms above `max_order`, on a short axis, or on the
`bare_base` of its bearing axis, and a bud older than the `branch_lifespan` of its
position is left out with all it would bear. A bud whose axis would be shorter than half
an internode aborts.

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
| `branch_lifespan` | curve, order of the bearing axis | along the bearing axis | number of cycles a lateral lives before it is shed with all it bears; 0 for ever. A lateral formed in cycle `c` is present while `age - c` is at most the lifespan |
| `aging_angle` | single curve | relative age of a lateral with a lifespan, 0 just formed, 1 about to be shed | angle added to `branching_angle`; a rising curve makes old fronds droop under young ones |
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
| `fork_limit` | integer | | largest number of successive forks along one line of descent: 1 gives at most two leaders, 2 at most four. Unlimited by default, in which case a fork probability applied over many cycles doubles the axes again and again |
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

`[growth.self_organization]` is read by the self-organizing mode only.

## Self-organizing mode

With `growth.mode = "self_organizing"` the same cycle runs with other stages: buds
compete for light and space, so a crown adapts to its own shade, to an envelope, to
obstacles and to neighbours. The model is that of Palubicki et al. (2009, section 4),
with the direction equation and the combination of markers and light of Longay et al.
(2012).

```rust
use arbrume_grow::{GrowParams, Occluder, Shape, Surroundings, grow_in};
use glam::Vec3;

let wall = Shape::Box { min: Vec3::new(1.5, 0.0, -8.0), max: Vec3::new(2.0, 9.0, 8.0) };
let surroundings = Surroundings { envelope: None, occluders: vec![Occluder::obstacle(wall)] };
let graph = grow_in(&species, &GrowParams { seed: 1, age: 20, scale: 1.0 }, &surroundings);
```

`grow` grows the plant on open ground, `grow_in` in given surroundings. A parametric
species ignores the surroundings.

| Stage | Implementation | What it does |
|---|---|---|
| 1 | `Competition` | exposure `Q` and direction `V` of every live bud, from a `ShadowGrid`, from `Markers`, or from both |
| 2 | `ResourceFate` | basipetal sum of `Q`, acropetal split of the resource `v`, shoots of `floor(v)` metamers |
| 3 | `Confined<TropismBlend>` | the heading equation above with `E = V`; keeps every metamer inside the envelope, out of the obstacles and above the ground |
| 4 | `ExposureShedding` | sheds the branches whose light per internode is too low; ends old dormant buds |

Lengths in `[growth.self_organization]` are in *internode units*, as in the paper: one
unit is the `internode_length` of order 1 at the base of an axis.

### Light: shadow propagation

Space is cut into cubic voxels of edge `shadow_cell`, each holding a shadow value `s`.
A metamer in voxel $(I, J, K)$ adds

$$
\Delta s = a\,b^{-q}
$$

to the voxels $(I \pm p,\ J - q,\ K \pm p)$ for $q = 0 \ldots q_{max}$ and
$p = 0 \ldots q$: a pyramid opening downwards whose layer $q$ is a full square of
$2q + 1$ voxels a side. The square follows fig. 5 of the paper, which draws each layer
filled; the printed index set, read literally with one $p$ for both $I \pm p$ and
$K \pm p$, would cover only the two diagonals of each layer. The exposure of a bud is

$$
Q = \max(C - s + a,\ 0)
$$

where the $+a$ cancels the shadow of the metamer that bears the bud, and its preferred
direction $V$ is the opposite of the gradient of $s$, taken by central differences.
$a$ is `shadow_strength`, $b$ `shadow_decay`, $q_{max}$ `shadow_depth` and $C$
`full_exposure`. Every metamer casts its pyramid when it appears and takes it back when
it is shed, so a cycle costs the new metamers, not the whole plant. Light comes from
above only: a crown is lit on its top and its upper flanks.

The grid (`ShadowGrid`) is only the field, with `cast`, `shadow`, `exposure` and
`gradient`; it knows nothing of plants, and several plants can write to one grid
(below).

### Space: markers

With `sensing = "markers"` the free space is a set of marker points scattered uniformly
in the crown envelope, `marker_density` per cubic internode unit and at most
`marker_limit`. Each cycle the markers within `occupancy_radius` of a bud are removed;
each other marker inside the perception cone of one or more buds (length
`perception_distance`, full angle `perception_angle_deg`, about the direction of the
bud) goes to the closest of them. A bud with markers has $Q = 1$ and

$$
V = \frac{n}{\lVert n \rVert}, \qquad n = \sum_{m \in S} \frac{m - p}{\lVert m - p \rVert}
$$

with $p$ the bud and $S$ its markers (Runions et al. 2007, eq. 2); a bud without markers
has $Q = 0$. Their eq. 3 adds a bias vector to this direction; that role is played here
by the $\beta T$ term of the heading equation. The envelope is that of the surroundings, or without one an ellipsoid standing
on the ground, as tall as the `axis_length` of the trunk and as wide as the longest
`axis_length` of order 1 on each side (`default_envelope`).

With `sensing = "both"`, a bud takes the $Q$ of the shadow model if it has markers and 0
otherwise, and the $V$ of its markers. This is the mode for shaping a crown with an
envelope: the markers fill the volume and the light thins the inside.

### Resource: Borchert-Honda and priority

Each cycle the exposures are summed from the tips to the base over the axes (resting
buds included), each stem receives

$$
v_{base} = \alpha\,Q_{base}
$$

with $\alpha$ the `resource_coefficient`, and the resource is split on the way up.

**Extended Borchert-Honda** (`allocation = "borchert_honda"`). The branching points of an
axis are taken from its base to its tip. At each one the resource $v$ in the axis is
divided between the continuation of the axis, which gathers the light $Q_m$, and the
lateral, which gathers $Q_l$:

$$
v_m = v\,\frac{\lambda Q_m}{\lambda Q_m + (1 - \lambda)\,Q_l}, \qquad
v_l = v\,\frac{(1 - \lambda)\,Q_l}{\lambda Q_m + (1 - \lambda)\,Q_l}
$$

$\lambda$ is `axis_bias`: 0.5 shares in proportion to the light, more favours the
leader (excurrent trees), less the laterals (decurrent trees). The paper shows the range
0.46 to 0.54; a few hundredths change the habit. `axis_bias` is a curve over the order
of the bearing axis and the position of the branching point, so the bias can differ
between trunk and branches or fade along an axis.

**Priority** (`allocation = "priority"`). The axes borne by an axis and its terminal bud
are ranked by the light they gather per bud; where `apical_control` is at least 0.5 the
terminal bud heads the list whatever its light. Item $i$ of $N$ receives

$$
v_i = v\,\frac{Q_i\,w_i}{\sum_{j=1}^{N} Q_j\,w_j}
$$

where the weight falls linearly with the rank from `priority_max` at the head of the
list to `priority_min` at the fraction `priority_knee` of its length and stays there.
The paper gives the weight function as a figure only; the linear ramp is the reading of
this implementation. `apical_control` is a curve over the order of the axis and the
position of its tip: letting it fall below 0.5 towards the end of the trunk gives a tree
that is excurrent when young and decurrent when old.

**Shoots.** A bud that receives $v \ge 1$ flushes $n = \lfloor v \rfloor$ metamers, each
$v / n$ times the `internode_length` at that place. A lateral bud flushes at the
earliest the cycle after it formed.

### What the gradient curves do in this mode

The species file is the same; the curves act as limits and biases.

| Field | Role in the self-organizing mode |
|---|---|
| `axis_length` | the length at which an axis stops; relative positions along an axis refer to it. Order 1 over the height of the trunk still shapes the crown |
| `shoot_length` | the longest shoot of one cycle: metamers above it are not built |
| `internode_length` | the internode a resource of 1 buys |
| `branch_probability`, `unit_branching`, `whorl` | where buds form, as in the parametric mode; a zero at the base of the trunk leaves a clear bole |
| `branching_angle`, phyllotaxis, `gravitropic_setpoint`, `tropism_weight`, `curvature`, `gnarl`, `direction_variation` | unchanged |
| `environment_weight` | weight $\gamma$ of $V$: how strongly shoots turn towards light or free space |
| `apical_dominance`, `parent_length_exponent`, `length_variation` | act on the limit `axis_length` |
| `bare_base`, `branch_lifespan`, `aging_angle` | not used: shedding does this |
| `branch_timing` | not used: buds are proleptic |
| forks, relays, stems, `weight_bending` | unchanged |

### Shedding

After each cycle, the light gathered by a branch (the exposures of the buds it bears
that can still grow, as sensed at the start of the cycle) is divided by its number of
internodes at the start of the cycle. Below `shedding_threshold` the branch is shed with
all it bears (Palubicki et al. 2009, section 4.4, after Takenaka 1994). Stems are never
shed. Shed metamers stay in the graph, flagged `SHED` with the cycle of their shedding
(`shed_cycles()`); the pipe model keeps counting them, so radii are not reduced by
shedding. The rule suits the shadow model; with markers alone `Q` is 0 or 1 and a branch
would be shed as soon as it stops.

A lateral bud that has not flushed `bud_lifespan` cycles after it formed dies. The paper
keeps buds for ever (0); a limit bounds the dormant buds a large plant carries, and the
time of every cycle with them.

### Parameters of `[growth.self_organization]`

| Field | Default | Meaning |
|---|---|---|
| `sensing` | `shadow` | `shadow`, `markers` or `both` |
| `allocation` | `borchert_honda` | `borchert_honda` or `priority` |
| `axis_bias` | 0.5 | $\lambda$, curve over the bearing order and the position of the branching point |
| `resource_coefficient` | 2 | $\alpha$ |
| `light_exponent` | 1 | exponent applied to $Q$ before it is summed; above 1 penalizes shaded buds (the light sensitivity of Longay et al. 2012) |
| `gravimorphism` | 1 | $d$ in $[0, 1]$: $Q$ is multiplied by $b = \cos^2\tau + d \sin^2\tau$, $\tau$ the angle of the internode bearing the bud from the vertical (Longay et al. 2012, eq. 1); 1 for $b = 1$ |
| `shadow_cell` | 1 | voxel edge, in internode units |
| `shadow_strength` | 0.2 | $a$ |
| `shadow_decay` | 2 | $b$, above 1 |
| `shadow_depth` | 6 | $q_{max}$, at most 16 |
| `full_exposure` | 1 | $C$ |
| `occupancy_radius` | 2 | radius in which a bud removes markers, in internode units |
| `perception_distance` | 4 | length of the perception cone, in internode units |
| `perception_angle_deg` | 90 | full angle of the perception cone |
| `marker_density` | 0.5 | markers per cubic internode unit of the envelope |
| `marker_limit` | 200000 | largest number of markers |
| `priority_max`, `priority_min` | 1, 0.006 | weights at the head and past the knee of a priority list |
| `priority_knee` | 0.5 | fraction of the list over which the weight falls |
| `apical_control` | 1 | curve over the order of the axis and the position of its tip; at least 0.5 puts the terminal bud first |
| `shedding_threshold` | 0 | exposure per internode below which a branch is shed; 0 for no shedding |
| `bud_lifespan` | 0 | cycles a lateral bud stays able to flush; 0 for ever |

Palubicki et al. (2009) give typical values for the occupancy radius (2 internodes), the
perception cone (4 to 6 internodes, about 90 degrees), the depth of the shadow (4 to 8
layers), $\alpha$ (2), $\lambda$ (0.46 to 0.54) and the priority weights (1, 0.006 and a
knee of 0.5 or 0.35): the defaults take those. The paper gives no value for $a$, $b$,
$C$ or the shedding threshold; those defaults, like `light_exponent`, `shadow_cell`,
`marker_density` and `bud_lifespan`, are starting points of this implementation. The
light sensitivity of Longay et al. (2012) is $Q = b\,L^\kappa$, $\kappa$ `light_exponent`
and $b$ the gravimorphism factor of their eq. 1; `gravimorphism` = 1 makes $b = 1$, the
plain $Q$ of Palubicki et al. (2009). Below 1 it favours the buds of upright axes over
those of horizontal ones.

The parameter set of Stava et al. (2014) is the reference for coverage. Its parameters
map to species fields as follows; the rules differ (their buds flush with a probability,
here with a resource), so their values do not carry over.

| Stava et al. (2014) | Here |
|---|---|
| apical angle variance | `direction_variation`, `gnarl` |
| number of lateral buds | `architecture.phyllotaxis.leaves_per_node`, `whorl` |
| branching angle mean and variance | `branching_angle`, `branching_angle_variation` |
| roll angle mean and variance | `architecture.phyllotaxis.divergence_deg`, `phyllotaxis_variation` |
| internode base length and its age factor | `internode_length` (a curve over position, not age) |
| growth rate | `resource_coefficient`, `shoot_length` |
| apical and lateral bud extinction | `bud_lifespan` for lateral buds; an apex ends at `axis_length` or with its branch |
| apical and lateral light factors | `light_exponent` (one for both) |
| apical dominance: base, distance and age factors | `axis_bias`, or `priority_*` |
| apical control and its age factor | `apical_control`, `apical_dominance` |
| phototropism | `environment_weight` |
| gravitropism | `gravitropic_setpoint`, `tropism_weight` |
| pruning factor | `shedding_threshold` |
| low branch pruning factor | `branch_probability` at the base of the trunk |
| gravity bending strength and angle | `weight_bending` |

### Surroundings

`Surroundings` holds what stands around one plant, in meters in the frame of the plant
(the plant stands at the origin, Y up; the scale of the plant does not apply):

```rust
pub struct Surroundings {
    pub envelope: Option<Shape>,   // volume the crown may fill
    pub occluders: Vec<Occluder>,  // volumes that block growth, cast shade, or both
}
pub struct Occluder { pub shape: Shape, pub blocks: bool, pub shade: f32 }
```

| `Shape` | Fields |
|---|---|
| `Sphere` | `center`, `radius` |
| `Ellipsoid` | `center`, `radii` along X, Y, Z |
| `Box` | `min`, `max` |
| `Cylinder` | `base` (centre of the bottom disc), `radius`, `height`; vertical |
| `Cone` | `base`, `radius`, `height`; vertical, apex up |
| `Voxels` | a `VoxelMask`: a grid of filled and empty cubes, the way to pass a voxelized mesh |

In TOML (CLI configs, `--surroundings` files) a shape is a table with a `shape` key;
`blocks` defaults to true and `shade` to 1. A voxel mask is built in code
(`VoxelMask::new`, `set`, `from_fn`).

```toml
envelope = { shape = "ellipsoid", center = [0.0, 5.0, 0.0], radii = [2.5, 5.0, 2.5] }

[[occluders]]                 # a wall
shape = "box"
min = [1.5, 0.0, -8.0]
max = [2.0, 9.0, 8.0]

[[occluders]]                 # the crown of a neighbour: shade only
shape = "ellipsoid"
center = [-5.0, 9.0, 0.0]
radii = [5.0, 6.0, 5.0]
blocks = false
shade = 4.0
```

**Blocking.** No metamer ends inside a blocking occluder, outside the envelope, or below
the ground for a shoot heading down. The fate stage allots only the metamers that fit
along the straight continuation of the axis; the orientation stage checks each metamer
where it really goes, tries it straight ahead if it is blocked, and ends the shoot
there if that is blocked too. A bud that cannot take one step is left out of the
allocation, so its share goes to the buds that have room: a plant pressed against a
clip volume branches behind it, the response to clipping of Prusinkiewicz et al.
(1994). A step that starts outside the envelope is allowed while it comes closer to
it, so a trunk can rise into a crown envelope that floats above the ground. The
distance to a voxel mask is that to the centre of its nearest filled voxel, read from
an exact distance transform of the mask (Felzenszwalb and Huttenlocher 2012) built the
first time it is needed, interpolated between voxel centres and continued outside the
grid; a floating voxel envelope is therefore reached as an analytic one is.

**Shade.** An occluder with `shade > 0` darkens the buds near and below it: a bud
receives `shade * a * b^-q` for every depth `q` in `0..=q_max` such that the volume
comes within `q` voxels of the point `q` voxels above the bud. This treats a solid as
one opaque layer per depth with one strength, a coarse rule of this implementation, not
a light simulation. A wall has `shade = 1`; the crown of a neighbour is an ellipsoid
with `blocks = false` and a larger `shade`. The shade of occluders also enters `V`: its
central difference over one voxel along each axis is added to the gradient of $s$, so
buds turn away from a shading wall as well as being darkened by it.

### Several plants: stands

`grow_together` grows a stand: several plants of any species, each at its position in
the frame of the surroundings, that shade one another.

```rust
let stand = [
    Standing { species: &oak, params, position: Vec3::new(-2.5, 0.0, 0.0) },
    Standing { species: &pine, params: other, position: Vec3::new(2.5, 0.0, 0.0) },
];
let graphs = grow_together(&stand, &Surroundings::none());
```

Each graph comes back in the frame of its own plant, standing at the origin, in the
order of the entries. A plant younger than the oldest starts later, so that all end
together. The plants grow in lockstep, and every cycle has three phases, each done for
all the plants before the next starts:

1. every self-organizing plant that senses light casts the shadow of its new metamers
   into one shared shadow grid, and takes back that of the metamers it shed;
2. every plant senses the grid, which then holds the shade of the whole stand as it was
   at the end of the previous cycle;
3. every plant allocates, grows its shoots and sheds, on its own; the plants of this
   phase run in parallel on the `rayon` pool.

No plant sees the growth of another within the same cycle, so the order of the entries
does not matter. Floating-point sums depend on their order, so the casts are summed in
an order fixed by the entries themselves (position, then seed, age, scale and species
name): permuting the entries gives the same plants to the bit. The number of threads
and the grid storage do not change them either.

**Each species casts its own shadow.** The grid has the voxels of the species with the
finest voxels (`shadow_cell` times the internode length and the scale). A plant casts
with the strength `a`, decay `b` and depth `q_max` of its own species; when its voxels
are larger than those of the grid, its pyramid is rescaled to the grid: `q_max` times
the ratio of the edges, rounded, and `b` raised to the inverse ratio, so that the
pyramid reaches as deep and fades as fast in meters
(`ShadowParams::in_cell`). A bud reads its exposure `Q = max(C - s + a, 0)` with the
`C` and `a` of its own species. A plant of a species with coarser voxels than the grid
reads shade on finer voxels than it would alone, so its growth is not exactly that of
the same plant alone even far from its neighbours.

**Markers are not shared.** A plant that senses space scatters its own markers: in the
envelope of the surroundings, or in its default envelope moved to where it stands. Two
crowns of space-colonizing plants can therefore overlap; they compete through light
only, when they also sense it (`sensing = "both"`).

**Parametric plants** are grown as by `grow`: they neither cast nor sense shade.

**One plant.** A stand of one plant at the origin is the plant grown alone, to the bit.
A plant elsewhere reads the grid in voxels aligned with the stand, not with itself, so
it differs slightly from the plant grown alone even when it has no neighbour: two plants
50 m apart do not shade each other, and still differ from the same plants grown alone.

**Cost.** Phase 1 is the sensing of the `Competition` stage, which casts before it
reads; a stand with more than one plant sensing light senses twice per cycle, once to
cast and once to read. Sensing is idempotent, so this costs time, not a different plant.
The grid covers the whole stand: with the dense storage its memory grows with the area
of the stand; `grow_together_with` takes `EngineOptions` (a sparse grid for a stand
spread over a large area).

`arbrume_gen::Generator::generate_group` grows a stand and meshes each plant; the
command line writes the stand of a config file (`[[stand]]`, see
[Command line](cli.md#stands)), and Python has `arbrume.generate_stand`.

### Age and growth sequences

The stages of this mode do not look at the age of the run. A plant grown for `n` cycles
is the metamers born before cycle `n` of the same plant grown longer, at the same
places, and the branches it has shed by then are those shed before cycle `n` of the
longer run. Radii and leaf sites depend on the age, as in the parametric mode.

Every metamer records its birth and, when shed, the cycle of its shedding, so one growth
holds every earlier state:

```rust
let record = arbrume_grow::grow_timeline(&species, &params, &surroundings);
let at_cycle_9 = record.at_cycle(&species, params.scale, 9);  // then mesh with max_cycle = Some(9)
```

`grow_timeline` keeps the leaf sites of every metamer (a `GrowthRecord`).
`at_cycle(c)` copies the graph, restores the branches shed after cycle `c`, sets the
radii the radius model gives on the structure of that cycle (`radii_at_cycle`: the pipe
model from the tips the plant had then, or the tapered model with the lengths of then;
one pass over the nodes), and keeps the leaves at the tip of the axes that had stopped by
then. The mesher then leaves out what was born later (`MeshOptions::max_cycle`). The
result has the live metamers of the plant grown to that age, with the same pipe radii.
Axes keep the position they have in the grown plant, so nothing jumps between frames;
with `weight_bending` they carry the sag of the final weight at every age, and an axis
that paused and grew again is treated as growing throughout for its leaves.
The record lists first the leaf sites the grown plant keeps, in its order, so a leaf has
the same index, and therefore the same place in the random leaf order of the mesher, in
every frame. At the last cycle (`is_last_cycle`) `at_cycle` returns the grown plant
itself, its radii and only its leaf sites, to be meshed without `max_cycle`: the last
frame is then the plant of `grow_in`, bit for bit.
`arbrume_gen::Generator::timeline` and `frame` wrap this, and `arbrume gen
--growth-frames K` writes the sequence ([Command line](cli.md)).

### Performance and engine options

`EngineOptions` holds the choices that change time and memory but never the plant; the
defaults come from the measurements below (`arbrume bench --engine`, and
`cargo bench -p arbrume-gen --bench engine`), taken on one 16-thread desktop: they
compare options, they are not targets.

- **Shadow grid storage.** `GridStorage::Dense` is one array over the bounding box of
  what was cast, enlarged when a pyramid leaves it; `GridStorage::Sparse` is a hash
  table of blocks of 8 x 8 x 8 voxels. Casting and reading the shadows of the 19 756
  nodes of one tree took 3.9 ms dense and 18.9 ms sparse, for 3.9 MB and 1.1 MB of
  voxels; whole growths are 1.2 to 2.1 times slower with the sparse grid. The default
  is dense.
- **Sensing buds in parallel** (`parallel_buds`). Reading the grid for each bud is a few
  memory reads; spreading it over the pool changed the growth time by -15 % to +50 %
  from one run to the next without a consistent gain. The default is sequential.
- **Giving markers to buds in parallel** (`parallel_markers`). The markers are bucketed
  in a hash grid and each bud visits the cells within reach of it, skipping those
  outside its occupancy zone and perception cone; when the buds are not fewer than the
  markers, the buds are bucketed and each marker looks them up instead. Each marker
  keeps its closest claiming bud, and the sums are then made on one thread in marker
  order, so the result does not depend on the threads nor on the way round. One cycle
  with 6 934 markers and 70 buds took 0.23 to 0.31 ms on one thread and 0.16 to
  0.18 ms on the pool, against 1.39 ms and 0.19 ms when every marker looked up the buds,
  measured on a loaded machine; in a batch that already uses every thread it changes
  nothing. The default is parallel.

Growth time of one plant, default options (median of 7, milliseconds):

| preset | age | nodes | default | sparse grid | parallel buds | sequential markers |
|---|---|---|---|---|---|---|
| `open_grown_broadleaf` | 20 | 3 958 | 3.1 | 4.2 | 3.6 | 3.0 |
| `open_grown_broadleaf` | 40 | 25 422 | 20.3 | 38.4 | 23.2 | 20.7 |
| `forest_grown_broadleaf` | 40 | 13 449 | 9.7 | 19.5 | 11.2 | 10.0 |
| `excurrent_conifer` | 40 | 10 167 | 9.0 | 14.2 | 10.3 | 8.6 |
| `spreading_decurrent` | 40 | 9 867 | 8.3 | 14.3 | 9.0 | 7.8 |
| `thicket_shrub` | 20 | 2 390 | 1.1 | 1.8 | 1.1 | 1.1 |
| `hedge_shrub` | 20 | 3 604 | 1.7 | 2.5 | 2.1 | 1.7 |
| `envelope_tree` | 20 | 1 905 | 4.6 | 5.8 | 4.9 | 12.9 |

Measured on an AMD Ryzen 7 9700F (8 cores, 16 logical processors) under Windows 11,
with no other build or benchmark running.

Growth costs about one microsecond per node on these presets, and stays below the cost
of meshing and export for the smaller plants ([Command line](cli.md) has the stage
table). The batch of plants stays the main parallel unit.

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

A node has `leaves_per_node` sites, at the same azimuths as its buds, or
`foliage.per_node` sites spread evenly around it when that is set: more sites than the
phyllotaxis gives thicken the foliage without adding buds. With `r` the
`foliage.retention_cycles`, a node keeps its leaves when it was born in the last `r`
cycles, or when it belongs to the last `r` shoots of an axis that has finished growing:
the tip of a full-grown axis stays leafy, as if it kept making short shoots. In the
self-organizing mode the same holds for an axis whose apex is alive but received too
little to extend: a stalled branch keeps its tuft until it is shed. For a
deciduous tree `r` counts the growth units whose short shoots still bear leaves, not the
life of one leaf, so 3 or 4 fills a crown where 1 leaves only tufts at the tips.

Two more fields select the sites. Axes of an order below `foliage.min_order` bear none
(1 keeps a trunk or a cane bare). Each position is kept with the probability
`foliage.density`, drawn from the key of the meristem, the index of the node and the
position, so thinning the foliage does not move the leaves that stay.

A site bears one leaf, or one twig when the species has a `[foliage.twig]`: a stem with
several leaves, or a needle-clad branchlet, built by the mesher (see
[Meshing](mesh.md)). With `foliage.inclination_deg = 0` the twig lies along the axis.

## Species

The preset library is described in [Species library](species.md).
`crates/arbrume-grow/tests/data/` holds a fixture of the self-organizing mode
(`competing.toml`) and three older species files kept as test fixtures: a
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
- Felzenszwalb and Huttenlocher 2012, Distance transforms of sampled functions, Theory
  of Computing.
- Longay, Runions, Boudon and Prusinkiewicz 2012, TreeSketch, Eurographics SBIM.
- Palubicki et al. 2009, Self-organizing tree models for image synthesis, ACM TOG.
- Prusinkiewicz, James and Mech 1994, Synthetic topiary, SIGGRAPH.
- Prusinkiewicz and Lindenmayer 1990, The Algorithmic Beauty of Plants, Springer.
- Prusinkiewicz, Mündermann, Karwowski and Lane 2001, The use of positional information
  in the modeling of plants, SIGGRAPH.
- de Reffye et al. 1988, Plant models faithful to botanical structure and development,
  SIGGRAPH.
- Runions, Lane and Prusinkiewicz 2007, Modeling trees with a space colonization
  algorithm, Eurographics Workshop on Natural Phenomena.
- Shinozaki, Yoda, Hozumi and Kira 1964, A quantitative analysis of plant form: the pipe
  model theory, Japanese Journal of Ecology.
- Stava et al. 2014, Inverse procedural modelling of trees, Computer Graphics Forum.
- Takenaka 1994, A simulation model of tree architecture development based on growth
  response to local light environment, Journal of Plant Research.
- Weber and Penn 1995, Creation and rendering of realistic trees, SIGGRAPH.
