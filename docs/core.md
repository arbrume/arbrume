# Core data model

`arbrume-core` holds the data every other crate works on: the plant graph, the species
description, curves, random numbers, a spatial grid and the mesh buffers. This page is the
reference for writing growth and meshing code on top of it. The rustdoc of each module has
the full signatures (`cargo doc -p arbrume-core --open`).

| Module | Content |
|---|---|
| `graph` | `PlantGraph`: metamers, chains (axes), buds, organ sites, traversals |
| `species` | `Species`: the TOML schema, in five layers |
| `curve` | `Curve` and `OrderCurve`: parameters as functions of position and order |
| `rng` | `Rng` and `Stream`: counter-based random numbers |
| `grid` | `HashGrid`: radius and cone queries over points |
| `mesh` | `TreeMesh`, `MeshBuffers`, `Material`: output of the mesher |

Conventions: meters, right-handed, Y up, `f32` geometry (`glam::Vec3`), angles in degrees
in species files and radians in code. Branching order 0 is the trunk.

## Plant graph

### Vocabulary

The terms are those of plant architecture (Barthélémy and Caraglio 2007).

- A **metamer** is one internode, the node at its distal end, and the leaves and buds on
  that node. One metamer is one **node** of the graph (`NodeId`).
- An **axis** is the sequence of metamers built by one meristem. It is called a **chain**
  in the code (`ChainId`, `Chain`).
- The **order** of a chain is 0 for the trunk and the parent's order plus one for a
  lateral chain (`ChainKind::Lateral`). A relay chain of a sympodial axis keeps the order
  of the chain it continues (`ChainKind::Relay`).
- A **bud** (`BudId`) is a meristem that may grow: the terminal bud at the tip of a chain,
  or a lateral bud on a node.
- An **organ site** (`OrganSite`) is a place where the mesher instances a leaf, twig,
  flower or fruit.

### What a node stores

Nodes are stored as one array per field, indexed by node id, without any allocation per
node. Each accessor returns the whole slice.

| Accessor | Type | Meaning |
|---|---|---|
| `parents()` | `&[NodeId]` | parent node, `NodeId::NONE` for the first node of a root chain |
| `positions()` | `&[Vec3]` | distal end of the internode (the node itself) |
| `radii()` | `&[f32]` | radius of the internode |
| `node_chains()` | `&[ChainId]` | chain the node belongs to |
| `orders()` | `&[u8]` | branching order (that of the chain) |
| `births()` | `&[u16]` | growth cycle in which the metamer appeared |
| `flags()` | `&[NodeFlags]` | `SHED`, `GROWTH_UNIT_START` |

The proximal end of an internode is not stored: it is the position of the parent node, or
the origin of the chain for the first node of a root chain (`proximal(node)`). The length
is derived (`internode_length(node)`). The first internode of a lateral chain therefore
starts on the centerline of the parent axis, at the attachment node.

`positions_mut()`, `radii_mut()` and `flags_mut()` give write access for passes such as
bending or radius computation.

### Ordering guarantees

1. **A parent node is stored before its children**, always. Iterating ids upward visits
   the plant from base to tips (acropetal); iterating downward visits it from tips to
   base (basipetal). No stack or child list is needed.
2. A parent chain has a smaller id than its child chains.
3. Chain ids, bud ids and organ indices never change. Node ids change only in
   `compact()`.

### Chains and contiguity

```rust
pub struct Chain {
    pub first: NodeId,       // most proximal node
    pub last: NodeId,        // most distal node
    pub len: u32,            // at least 1
    pub attach: NodeId,      // node of the parent chain, NONE for a root chain
    pub order: u8,
    pub origin: Vec3,        // base point of a root chain only
    pub terminal_bud: BudId, // NONE when there is none
}
```

The mesher needs each chain as a contiguous slice. Growth appends nodes at the end of the
arena in constant time, which gives this rule:

- starting a chain keeps the layout contiguous;
- extending the chain that owns the last node of the arena keeps it contiguous;
- extending any other chain clears `is_contiguous()`.

A generator that completes one axis before starting the next, as the parametric mode
does, keeps the layout contiguous at no cost. A self-organizing run that extends many
chains per cycle breaks it, and restores it with one call before meshing:

```rust
if let Some(remap) = graph.ensure_contiguous() {
    // node ids changed: remap[old.index()] is the new id
}
```

`compact()` reorders the arena into depth-first order (below) and returns the mapping
from old to new node id, or `None` when nothing moved. Buds, organ sites and chains inside
the graph are updated; node ids held elsewhere must go through the mapping.
`ensure_contiguous()` calls `compact()` only when needed. Both are deterministic: the
result depends on the graph alone.

In a contiguous layout `chain_range(chain)` is the range of the chain in every node
slice. In any layout, `chain_nodes_rev(chain)` walks a chain from tip to base and
`chain_length(chain)` gives its arc length.

### Depth-first order

`depth_first_order()` lists the nodes in the order used for serialization (the ordering
Wang et al. 2025 found best for learning on trees). From a node, the traversal follows the
continuation of its chain to the tip, then visits the chains attached to the node. So:

- each chain is contiguous, base to tip;
- the subtree of any node is a contiguous run starting at that node;
- the chains attached to an axis come from its tip down to its base;
- parents precede children.

After `compact()` the arena itself is in this order.

### Growing

```rust
let base = graph.add_root_chain(origin, Metamer::new(position, radius, cycle));
let trunk = graph.chain_of(base);
let tip = graph.extend_chain(trunk, Metamer::new(position, radius, cycle));
let side = graph.add_chain(attach_node, ChainKind::Lateral, metamer);
```

| Method | Effect |
|---|---|
| `add_root_chain(origin, metamer) -> NodeId` | new order 0 chain starting at `origin`; several are allowed |
| `add_chain(attach, kind, metamer) -> NodeId` | new chain attached to a node |
| `extend_chain(chain, metamer) -> NodeId` | one more metamer at the tip; the terminal bud follows |
| `add_bud(node, kind, direction) -> BudId` | dormant bud; a terminal bud goes on the tip of a chain without one |
| `sprout(bud, kind, metamer) -> NodeId` | a lateral bud becomes a chain and stays as its terminal bud |
| `shed_subtree(node)` | flags the node and its descendants as shed, kills their buds |
| `shed_subtrees(&nodes, cycle)` | the same for several branches in one pass, recording the growth cycle of the shedding |
| `restore_shed_after(cycle)` | clears the flag of the nodes shed after `cycle`: the branches the plant still bore then |
| `add_organ(site) -> usize`, `clear_organs()` | organ sites |

A `Metamer` is `{ position, radius, birth, flags }`. `birth` must not be earlier than the
parent's. Preconditions are checked with `debug_assert!`; `check_invariants()` verifies
the whole structure and is meant for tests.

Buds are optional. The parametric mode can build a plant with `add_chain` and
`extend_chain` alone.

### Buds

Buds are also stored as arrays. Each has a bearing node, a kind (`Terminal`, `Lateral`), a
state (`Dormant`, `Active`, `Dead`), a unit growth `direction`, and the scalars of the
growth cycle of Palubicki et al. (2009): exposure `Q`, preferred direction `V` and
resource `v`. A bud sits at the position of its node (`bud_position`).

Read with `bud(id)` (a copy of all fields) or the slices `bud_nodes()`, `bud_kinds()`,
`bud_states()`, `bud_directions()`, `bud_preferred()`, `bud_exposures()`,
`bud_resources()`. Write with `buds_mut()`, which lends every writable field at once
together with the node positions, so one pass (possibly parallel over buds) can fill
exposure and preferred direction:

```rust
let buds = graph.buds_mut();
for (bud, exposure) in buds.exposure.iter_mut().enumerate() {
    let position = buds.node_positions[buds.nodes[bud].index()];
    *exposure = light_at(position);
}
```

Dead buds keep their slot, so bud ids stay valid for the life of the graph.

### Traversal helpers

```rust
// One value per node. combine(parent, child) runs once per node, tips first.
graph.accumulate_basipetal(&mut values, |parent, child| *parent += *child);
// derive(parent, child) runs once per node, base first.
graph.propagate_acropetal(&mut values, |parent, child| *child = *parent + 1.0);
```

`accumulate_basipetal` is the pass behind light sums, subtree sizes and the pipe model.
`pipe_radii(exponent, tip_radius)` applies the pipe model `r^n = sum of r_child^n`
directly to the stored radii. Shed nodes still count, so a branch keeps its girth after
losing sub-branches.

`children()` builds child lists for all nodes in one linear pass, for code that needs
them; the result is a snapshot. `is_ancestor(a, n)` walks the parent links.

### Shedding and time

Shed nodes stay in the graph with `NodeFlags::SHED`. A shed node has only shed
descendants, so within a chain the shed nodes are at the distal end. `shed_cycles()`
gives, for each shed node, the growth cycle in which it was shed (0 when it was shed
with `shed_subtree`), so the plant can be shown with the branches it bore at an earlier
age ([Growth](growth.md)).

Births never decrease from parent to child. The plant as it was at cycle `c` is the set
of nodes with `birth <= c`, which is a prefix of every chain.

### What the mesher reads

`chain_view(chain)` returns, for a contiguous layout:

```rust
pub struct ChainView<'a> {
    pub chain: ChainId,
    pub order: u8,
    pub attach: NodeId,             // NONE for a root chain
    pub origin: Vec3,               // start of the axis, on the parent centerline
    pub parent_radius: Option<f32>, // parent internode radius at the attachment
    pub first: NodeId,              // id of positions[0]
    pub positions: &'a [Vec3],      // live nodes, base to tip
    pub radii: &'a [f32],
    pub births: &'a [u16],
}
```

The axis to sweep is the polyline `origin, positions[0], positions[1], ...`, and
`radii[i]` is the radius of the internode that ends at `positions[i]`. Shed nodes are left
out, so a view can be empty. `born_by(cycle)` gives the number of leading nodes that exist
at a given cycle, `length()` the arc length, `node(i)` the id of a node of the view.

Since chains are independent slices, they can be meshed in parallel: vertex counts per
chain give offsets by a prefix sum, and each chain writes its own part of the buffers.

Organ sites (`organs()`) carry a node, a kind, a unit `direction`, a unit `normal`
orthogonal to it, a `scale` and a birth cycle. The site is anchored on the axis at the
node position; moving it to the bark surface is the mesher's job. Skip sites whose node
is shed (`is_shed(site.node)`).

`bounds()` is the bounding box of all axis points (radii and organs not included).

## Random numbers

Every draw is a pure function of `(seed, node id, stream, counter)`:

```rust
let mut rng = Rng::new(seed, node.0, Stream::BRANCH_ANGLE);
let angle = rng.range(30.0, 60.0);
```

`Rng::new` builds a 24-byte cursor at counter 0; each draw advances the counter.
`Rng::at(seed, node, stream, counter)` starts at a given counter. Use one `Stream` per
kind of decision so that adding a draw to one decision does not shift another. Named
streams cover the growth and mesh stages; `Stream::custom(n)` gives ids that cannot
collide with them.

Draws: `next_u64`, `next_u32`, `f32` in `[0, 1)`, `signed` in `[-1, 1)`, `range(lo, hi)`,
`angle`, `below(n)`, `chance(p)`, `normal`, `normal_with(mean, sd)`, `unit_sphere`,
`unit_cone(axis, half_angle)`.

Rules that keep generation deterministic:

- Never share one cursor between nodes or threads. Build it where it is used.
- Key by the id of the entity that decides: the node bearing a bud, or the bud id.
- Node ids change in `compact()`. Compaction is deterministic, so results stay
  reproducible as long as the points where it is called do not depend on thread count or
  timing. Bud ids and chain ids never change and are safe keys across compaction.

The generator is `SplitMix64` (Steele, Lea and Flood 2014) with a hashed key and a per-key
increment; the module documentation describes the mixing function.

## Curves

Species parameters are functions of a relative position `x` in `[0, 1]` along an axis
and of the branching order (Prusinkiewicz et al. 2001).

- `Curve::eval(x)`: a constant, or control points joined linearly or by a monotone cubic
  (`Interpolation::Smooth`), which has a continuous slope and never overshoots its
  control points. Outside the first and last control point the curve keeps their value.
- `OrderCurve::eval(order, x)`: one curve per order; orders past the end of the list use
  the last curve.
- `bounds()` on both returns the exact minimum and maximum.

Evaluation does not allocate. A constant costs one branch; a control-point curve scans
its few points.

In TOML:

```toml
a = 0.5                                                  # constant
b = [[0.0, 1.0], [0.6, 0.8], [1.0, 0.1]]                 # linear control points
c = { points = [[0.0, 1.0], [1.0, 0.0]], interp = "smooth" }
d = { by_order = [60.0, [[0.0, 70.0], [1.0, 40.0]]] }    # order 0, then order 1 and up
```

## Species

A `Species` is loaded with `Species::from_path` or `Species::from_toml_str`, which also
validate it, and written with `to_toml_string`. Every field has a default and unknown
keys are errors. The defaults describe a plain generic tree; they are starting points for
tuning, not measurements.

Each layer lives in its own file under `crates/arbrume-core/src/species/` and validates
its own fields, so layers can be extended independently.

| Table | File | Fields |
|---|---|---|
| `[architecture]` | `architecture.rs` | `branching` (monopodial, sympodial), `growth_rhythm` (rhythmic, continuous), `branching_pattern` (rhythmic, continuous, diffuse), `branch_timing` (sylleptic, proleptic), `max_order`, `phyllotaxis` (`leaves_per_node`, `divergence_deg`), `axes` (per order: `orientation`, `short`, optional `phyllotaxis`) |
| `[growth]` | `growth.rs` | `mode` (parametric, self_organizing), lengths and extension, branching, shape of an axis, forks, relays, stems, shedding, bending, and the `[growth.self_organization]` table: every field is listed in [Growth](growth.md) |
| `[geometry]` | `geometry.rs` | `radius_model` (pipe, tapered), `pipe_exponent`, `tip_radius`, `radius_ratio`, `child_radius_exponent`, `taper`; `flare`, buttress, collar and `radial_segments` are listed in [Meshing](mesh.md) |
| `[foliage]` | `foliage.rs` | which nodes bear leaves (`enabled`, `retention_cycles`, `density`, `per_node`, `min_order`, see [Growth](growth.md)); the leaf, its variation and the `[foliage.twig]` instanced at each site (see [Meshing](mesh.md)) |
| `[bark]` | `bark.rs` | `color`, `young_color`, `young_radius`, `color_variation`, `roughness`, `texture_size`, `texture` (see [Meshing](mesh.md)) |
| `[variation]` | `variation.rs` | `age` and `scale` ranges, `weight`, and the `[variation.jitter]` table (below) |

Growth curves take one of two positions, stated in the documentation of each field:
*along the parent* (where the axis is attached, 0 at the parent's base) or *along the
axis* (where the metamer lies on its own axis). Acrotony and basitony are shapes of these
curves, not flags.

### Variation

`[variation]` says how individuals of a species differ, for generators that draw random
plants: `age = [young, old]` is the range of ages worth generating, `scale = [small,
large]` the range of overall scale, `weight` the relative frequency of the species in a
random mix (0 leaves it out). `[variation.jitter]` maps parameter names to a relative
half-width in `[0, 1)`:

```toml
[variation.jitter]
"growth.axis_length" = 0.15
"foliage.color" = 0.1
```

`species.varied(seed, index)` returns an individual: every jittered parameter is
multiplied by a factor drawn uniformly in `[1 - a, 1 + a)` and clamped to its domain. A
curve per order draws one factor per order (`OrderCurve::scaled`), a colour one per
channel; `bark.color` also scales `young_color` by a common factor. The draws depend on
the seed, the index and the name of the parameter only, so adding a parameter to the
table does not change the others. The names accepted are those of
`species::JITTERABLE`: `growth.axis_length`, `shoot_length`, `internode_length`,
`branch_probability`, `branching_angle`, `gravitropic_setpoint`, `tropism_weight`,
`curvature`, `gnarl`, `direction_variation`, `bare_base`, `fork_probability`,
`apical_dominance`, `stems`, `stem_angle`, `stem_spread`; `geometry.radius_ratio`,
`flare`; `foliage.length`, `width_ratio`, `density`, `inclination_deg`, `color`,
`twig.length`, `twig.bend_deg`; `bark.color`; `growth.self_organization.axis_bias`,
`resource_coefficient`, `shedding_threshold`. Any other name is an error.

To add a parameter: add the field with its doc comment to the struct of its layer, give
it a value in `Default`, and check its domain in that layer's `validate`. Because of
`#[serde(default)]`, existing species files keep loading.

`crates/arbrume-core/tests/data/example_broadleaf.toml` is a complete example file.

## Spatial grid

`HashGrid::build(points, cell_size)` indexes a slice of points in cubic cells stored in a
hash table: no world bounds, memory proportional to the point count, three flat arrays.
`rebuild` reuses the allocations.

```rust
grid.for_each_in_radius(center, radius, |index, point, distance_squared| { /* ... */ });
grid.for_each_in_cone(apex, axis, half_angle, range, |index, point, distance_squared| { /* ... */ });
let nearest = grid.nearest_in_radius(center, radius); // Option<(index, distance_squared)>
```

`index` is the position of the point in the slice given to `build`. Queries take `&self`
and can run in parallel. A cell size close to the usual query radius is a good start.
