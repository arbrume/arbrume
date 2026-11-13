# Meshing

`arbrume-mesh` turns a grown `PlantGraph` and its `Species` into a `TreeMesh`: a bark part
(trunk and branches) and a leaf part, each with its material. This page describes what the
mesher builds and every parameter that shapes it. The rustdoc of the crate has the
signatures (`cargo doc -p arbrume-mesh --open`).

```rust
use arbrume_mesh::{MeshOptions, mesh_tree};

let mesh = mesh_tree(&graph, &species, &MeshOptions::default())?;
```

The graph must be contiguous (see [Chains and contiguity](core.md)); otherwise
`mesh_tree` returns `MeshError::NotContiguous`. Call `graph.ensure_contiguous()` first
when the generator does not guarantee it.

| Entry point | Use |
|---|---|
| `mesh_tree(graph, species, options)` | one plant; allocates its working memory and its output |
| `mesh_counts(graph, species, options)` | vertex, triangle and leaf counts without building the mesh |
| `Mesher::new()`, then `mesher.mesh(...)`, `mesher.mesh_into(..., &mut mesh)`, `mesher.counts(...)` | many plants; keeps the working memory, and with `mesh_into` the output buffers |
| `synthetic::skeleton(params)` | hand-built test skeletons, for tests, benchmarks and demos |

The output depends on the graph, the species and the options only. It is bit-identical
with and without `MeshOptions::parallel`, for any number of threads, and whether or not a
`Mesher` is reused.

## Options

`MeshOptions` holds the resolution, the level of detail, the age and the threading.

| Field | Default | Meaning |
|---|---|---|
| `tolerance` | 0.002 m | largest distance allowed between the mesh and the smooth surface it approximates |
| `min_radius` | 0 | axes whose base radius is below this are not meshed, nor are the axes they bear; their leaves stay |
| `leaf_fraction` | 1 | fraction of the leaves (or twigs) kept, enlarged so that the total leaf area is preserved |
| `max_cycle` | none | mesh the plant as it was at the end of this growth cycle |
| `seed` | 0 | seed of the mesh-level variation (leaf order, size, attitude and colour, texture offsets) |
| `parallel` | false | fill the buffers on the global `rayon` pool |

`MeshOptions::with_quality(q)` sets the level-of-detail controls from one number, 1 being
full detail: the tolerance is the default divided by `q` squared and `leaf_fraction` is
`q`. Side, ring and leaf counts are then roughly proportional to `q`. `min_radius` stays
0: a thin axis costs few triangles at a coarse tolerance, and dropping it would leave its
leaves in the air. The defaults are starting points, not measured values.

## Branch sweep

Each chain is swept as a generalized cylinder along the polyline `origin, positions[0],
positions[1], ...` of its `ChainView`.

**Centerline.** Between two consecutive nodes the centerline is a cubic Hermite segment.
The tangent at a node is the bisector of the two chords meeting there, with the length of
the chord being drawn as its magnitude. The curve passes through every node, has a
continuous direction, and stays close to the polyline: it cannot loop or swing wide the
way a uniform Catmull-Rom spline does when internode lengths differ.

**Rings.** Cross-sections are placed by the tolerance `e`:

- a segment of length `L` turning by the angle `a` deviates from its chord by about
  `L a / 8`, so it gets `ceil(sqrt(L a / (8 e)))` rings;
- a node gets no ring when the nodes skipped since the last ring stay within `e` of a
  straight line and their radii within `e` of the radius at that ring. A straight twig
  costs two rings however many internodes it has;
- a ring of radius `r` gets `n = ceil(pi sqrt(r / (2 e)))` sides, the count for which a
  regular polygon stays within `e` of its circle, kept within the species bounds
  `geometry.radial_segments`. The count changes along an axis as it tapers.

**Radius.** The radius at a node is that of the internode ending there and varies
linearly between rings. The surface at a node is therefore exactly as wide as the graph
says, which is what leaves and child axes are placed against.

**Frame.** The direction from which angles around a ring are counted is carried from
ring to ring by parallel transport: it is turned by the smallest rotation that takes the
previous tangent onto the new one (Prusinkiewicz et al. 2001, eq. 10, with no rotation
about the heading). The surface does not twist however the axis bends, and unlike the
Frenet frame this frame is defined on straight parts and does not flip at inflection
points. The first direction points as far down as the tangent allows, which puts the
texture seam on the underside of a branch.

**Surface.** A ring of `n` sides has `n + 1` vertices, the first and the last at the same
place with texture coordinates one repeat apart. Two consecutive rings are joined by a
band of triangles; when their side counts differ the two rings are zipped, each vertex of
the coarser ring fanning out to the nearest sides of the finer one. Normals are those of
the smooth surface: radial, tilted by the slope of the radius, so collars and flares
shade as curved surfaces. Front faces are counter-clockwise seen from outside.

**Tip.** The tip of every axis is closed by a fan to one apex vertex half a radius beyond
the last ring.

**Foot.** The base of a root axis is closed by a flat fan facing backwards, with its own
copy of the first ring and a vertex at its centre, so a stem is not hollow seen from
below.

## Junctions

A lateral axis starts on the centerline of its parent, inside it, so the two tubes
intersect and no gap can open (as in Longay et al. 2012 and Raistrick et al. 2023). Where
the axis comes out of its parent it is widened into a collar that narrows back to the
radius of the axis, which hides the seam and reads as a branch collar.

The collar multiplies the radius by `1 + collar_flare` from the start of the axis to the
point where it crosses the parent surface, then by `1 + collar_flare (1 - x)^2` over
`collar_length` radii, `x` running from 0 to 1. It is never wider than the parent, so it
cannot come out on the far side, and it is left out when the widening is below the
tolerance. A relay axis, which continues its parent at the same order, gets no collar.

The junction is not watertight: the child is a separate tube that passes through the
surface of its parent.

## Trunk base

A root axis is widened near the ground by `geometry.flare`, following the profile of
Weber and Penn (1995), `Geometry::flare_factor`: the radius is multiplied by
`1 + 0.99 flare` at the ground and the widening vanishes at one eighth of the length of
the axis. Rings are added over that part so the profile is followed within the tolerance.

With `geometry.buttress_lobes > 0` the widening varies around the trunk: at the angle
`a` it is multiplied by `1 + buttress_depth sin(lobes a)`. Lobes modulate only the flare,
so they fade out with it. A lobed ring has at least four sides per lobe. Lobes whose
amplitude is below the tolerance are left out.

## Bark texture coordinates

`u` runs around the axis and `v` along it, in units of one texture repeat.

- `u` covers a whole number of repeats, `max(1, round(circumference / around))`, where
  the circumference is taken at the base of the axis and `[around, along]` is
  `bark.texture_size`. A whole number makes the texture wrap without a seam.
- `v` is the arc length along the node polyline times a scale chosen so that a texel is
  as long as it is wide at the base of the axis, in the proportion `around : along`.
- `v` starts at a random offset on each lateral axis, so that neighbouring branches do
  not show the same piece of bark.

Texel density is that of the species wherever the circumference is at least one texture
repeat. Thinner axes have one repeat around and a higher density. Along a tapering axis
the density rises towards the tip, since the number of repeats around is constant.

## Bark colour

The bark material takes `bark.roughness` and a base colour. A species that sets neither
`bark.young_color` nor `bark.color_variation` has no vertex colours on its bark, and the
base colour is `bark.color`.

- `young_color` is the colour of thin axes: the bark blends from it to `color` as the
  radius grows from 0 to `young_radius`, along a smoothstep. A birch has a white trunk and
  dark twigs this way, a pine a grey-brown trunk and orange limbs.
- `color_variation` darkens each axis by a random fraction up to this value, drawn from
  the mesh seed and the chain.

Both are written as vertex colours, constant around a ring. The base colour of the
material is then, per channel, the larger of `color` and `young_color`, and the vertex
colours are the ratio to it, so they never exceed 1.

## Foliage

One leaf, or one twig for a species with a `[foliage.twig]`, is instanced at every
`OrganSite` of kind `Leaf` whose node is not shed. Other kinds of organ are ignored for
now.

**Leaf geometry.** The species leaf is built once per mesh, in the frame of a site: `x`
from base to tip, `y` across, `z` towards the upper side.

- `geometry = "outline"`: a strip of triangles on both sides of the midrib, bounded by
  the blade outline. The half-width at the relative position `t` is proportional to
  `t^a (1 - t)^b`; `shape` chooses the exponents (`elliptic`, `ovate`, `obovate`,
  `lanceolate`, `needle`). Each half is raised about the midrib by `fold_deg` and the
  midrib bends along a circular arc by `curvature_deg`. A blade of `n` segments has
  `3 n - 1` vertices and `4 (n - 1)` triangles; `n` is `segments` at most, fewer when the
  tolerance is coarse. Whatever `n`, the polygon is widened so that its area is that of
  the smooth outline. A blade of two segments whose fold and bend both stay within the
  tolerance is drawn flat, as a diamond of two triangles. With `segments = 1` a blade is
  a single flat triangle, widest at its base: a needle.
- `leaflet_pairs > 0`: a pinnately compound leaf, `2 pairs + 1` blades on a straight
  rachis, the lateral ones at `leaflet_angle_deg` from it.
- `geometry = "card"`: a flat rectangle over the extent of the leaf, for an alpha-masked
  texture. The leaf material then has an alpha cutoff of 0.5; the caller attaches the
  texture to it.

Both kinds have texture coordinates over the bounding rectangle of the leaf, `v` from
base to tip. The petiole (`petiole_length`) and the rachis are one thin triangle; the
petiole of a simple leaf is left out when it is thinner than the tolerance. The leaf
material is double-sided.

**Twigs.** With `foliage.twig.leaves > 0` (or fins) a site bears a twig instead of a
leaf: a small template built once per mesh and instanced like a leaf, baked into the
leaf part (glTF instancing is not used). A twig lives in the frame of its site, `x`
along it, and its stem follows a circular arc of `twig.length` that bends by
`twig.bend_deg` towards the lower side of the site.

- *Leaves.* `twig.leaves` copies of the species leaf: the lateral ones on nodes evenly
  spaced along the stem, `twig.leaves_per_node` per node, turned by
  `twig.divergence_deg` from node to node and leaving the stem at
  `twig.inclination_deg`; a terminal one continues the stem (`twig.terminal_leaf`).
  Their upper sides face the upper side of the site, or with `twig.radial` their planes
  contain the stem, as for needles around a shoot.
- *Stem.* A three-sided pyramid of radius `twig.stem_radius` at the base, in two segments
  when the twig bends; none when the radius is 0.
- *Fins.* `twig.fins` flat strips along the stem, `twig.fin_width` wide on each side,
  darkened by `twig.fin_shade`. Needles are too many and too thin to draw: the fins stand
  for their bulk at two triangles each, and a few enlarged needles fringe the outline.
- *Variants.* A plant uses `twig.variants` versions of the template, which differ in the
  size, attitude and shade of each leaf (by the variation amounts of the species). Each
  site draws its variant. All variants have the same vertex count.

The stem belongs to the leaf part and takes the colour of young bark: the base colour of
the leaf material is, per channel, the larger of `foliage.color` and that colour, and the
vertex colours carry the ratio. With `foliage.inclination_deg = 0` a twig lies along the
axis that bears it and needs no stem: this is how the needles of a pine shoot are drawn.

A twig of 5 leaves of two segments at the default tolerance has 44 vertices and 28
triangles (8 vertices and 5 triangles per leaf with its petiole, 4 and 3 for the stem); a
bent conifer branchlet of 12 needles and 3 fins has 61 vertices and 33 triangles.

**Placement.** The leaf takes the frame of its site: `direction`, `normal x direction`,
`normal`. The site is on the centerline, so the leaf is moved out by the radius of its
node along the part of `direction` that is orthogonal to the internode. Its length is
`foliage.length` times the `scale` of the site.

**Variation.** Each leaf or twig draws its own size (`size_variation`), a small rotation about
each of its axes (`orientation_variation_deg`) and a colour (`hue_variation`,
`brightness_variation`) from the random stream of its organ index, so a leaf looks the
same whatever else changes in the plant. The colour is written as a vertex colour, at
most 1 per channel, that multiplies the base colour of the material (`foliage.color`,
unless the stem of a twig raises it, see above).

## Level of detail

Detail is reduced from the graph, not by decimating a mesh.

- **Tolerance.** A larger tolerance gives fewer rings, fewer sides and coarser blades,
  and leaves out collars, flares and lobes smaller than itself.
- **Radius cut.** Axes below `min_radius` are dropped with everything they bear. Their
  leaves stay, which keeps the crown full at a distance but leaves them in the air when
  seen closer; `with_quality` therefore does not use it.
- **Leaf prefix.** Leaves are stored in a random order, so any prefix of the leaf buffer
  is an unbiased sample of the crown (the stochastic simplification of Cook et al. 2007,
  used for foliage by Livny et al. 2011 and Pirk et al. 2012). `leaf_fraction = f` keeps
  the first `ceil(f n)` leaves and multiplies their size by `sqrt(n / kept)`, which
  preserves the total leaf area. The order of a leaf depends on the seed and on its organ
  index only: the leaves of a lower level are exactly the first leaves of a higher one,
  and the order does not change when the plant is cut at an earlier age.

`mesh_counts` gives the sizes for a set of options at a fraction of the cost of meshing,
to choose a level that fits a triangle budget.

## Age

`max_cycle = Some(c)` meshes the plant as it was at the end of cycle `c`: each chain is
cut to its nodes born by `c` (`ChainView::born_by`), and a leaf is kept when its site and
node are born by `c` and it is not older than the retention of the species
(`c - birth < foliage.retention_cycles`). Radii are those stored in the graph, so a
younger plant keeps the girth of the grown one unless the caller recomputes them.

Without `max_cycle`, every leaf site of the graph gets a leaf.

## Passes and threading

1. **Plan** (sequential). Every kept chain is sampled into rings, in one buffer for the
   whole plant, and its vertices and indices are counted. A running sum gives each chain
   its place in the output. Leaves are selected and ordered by a counting sort.
2. **Fill**. The buffers are sized once. Each chain writes its own slice from its rings
   alone, and each block of leaves its own slice. With `parallel`, the list of chains is
   halved by vertex count until a piece is small enough for one task, and leaf blocks are
   distributed likewise.

Nothing is allocated per node, per chain or per leaf. A batch of plants is best
parallelized by the caller, one `Mesher` per thread; the `parallel` switch is for a
single large plant. `cargo bench -p arbrume-mesh` measures both paths, with fresh and
with reused buffers.

## Species parameters

All defaults are starting points for tuning, not measured values.

### `[geometry]`

| Field | Default | Meaning |
|---|---|---|
| `flare` | 0.5 | widening of the trunk near the ground, 0 for none |
| `buttress_lobes` | 0 | number of lobes around the flared base, 0 for a round base (at most 16) |
| `buttress_depth` | 0.3 | depth of the lobes in `[0, 1]`, as a fraction of the flare widening |
| `collar_flare` | 0.35 | widening of a lateral axis where it leaves its parent, as a fraction of its radius |
| `collar_length` | 3.0 | length of the collar, in radii of the axis |
| `radial_segments` | `[3, 16]` | fewest and most sides of a cross-section (3 to 64) |

### `[foliage]`

| Field | Default | Meaning |
|---|---|---|
| `geometry` | `"outline"` | `"outline"` mesh or `"card"` rectangle |
| `shape` | `"ovate"` | blade outline: `elliptic`, `ovate`, `obovate`, `lanceolate`, `needle` |
| `segments` | 4 | most segments along the midrib of a blade (2 to 16), or 1 for a single triangle |
| `fold_deg` | 15 | angle by which each half of the blade is raised about the midrib |
| `curvature_deg` | 25 | bend of the blade from base to tip, positive towards its lower side |
| `petiole_length` | 0.15 | petiole length divided by the leaf length |
| `leaflet_pairs` | 0 | pairs of lateral leaflets of a compound leaf, 0 for a simple leaf (at most 32) |
| `leaflet_angle_deg` | 55 | angle between a lateral leaflet and the rachis |
| `size_variation` | 0.15 | relative spread of leaf sizes, in `[0, 1)` |
| `orientation_variation_deg` | 12 | largest random rotation of a leaf about each of its axes (at most 45) |
| `hue_variation` | 0.08 | opposite scaling of the red and blue channels of a leaf, in `[0, 1]` |
| `brightness_variation` | 0.25 | largest darkening of a leaf, in `[0, 1]` |
| `roughness` | 0.6 | roughness of the leaf material |

`length`, `width_ratio`, `color` and `retention_cycles` are used as described above. For
a compound leaf `length` is that of the whole leaf and `width_ratio` that of a leaflet.

`density`, `per_node` and `min_order` select the leaf sites during growth
([Growth](growth.md)).

### `[foliage.twig]`

| Field | Default | Meaning |
|---|---|---|
| `leaves` | 0 | leaves on one twig, 0 for none (at most 64) |
| `length` | 0.12 m | length of the twig stem |
| `leaves_per_node` | 1 | leaves at each node of the twig |
| `divergence_deg` | 137.5 | turn between successive nodes: 180 for two flat ranks, 0 with two leaves per node for a feather |
| `inclination_deg` | 50 | angle between a leaf and the twig |
| `bend_deg` | 15 | bend of the twig from base to tip, positive towards the lower side of the site |
| `stem_radius` | 0.002 m | radius of the stem at its base, 0 for no stem |
| `terminal_leaf` | true | a leaf continues the twig at its tip |
| `radial` | false | leaf planes contain the stem, and the first leaf points to a random azimuth |
| `fins` | 0 | flat strips along the stem (at most 8) |
| `fin_width` | 0.02 m | half-width of a fin |
| `fin_shade` | 0.8 | brightness of the fins relative to the leaves |
| `variants` | 4 | versions of the template used by one plant (1 to 16) |

### `[bark]`

| Field | Default | Meaning |
|---|---|---|
| `color` | `[0.2, 0.14, 0.09]` | colour of old bark, linear RGB |
| `young_color` | none | colour of thin axes |
| `young_radius` | 0.03 m | radius from which the bark has its old colour |
| `color_variation` | 0 | largest darkening of one axis, in `[0, 1]` |
| `roughness` | 0.9 | roughness of the bark material |
| `texture_size` | `[0.5, 0.5]` m | size of one texture repeat, around and along |

## Looking at a mesh

```
cargo run --release -p arbrume-mesh --example demo_tree -- tree.glb [quality] [seed] [species.toml]
```

writes the mesh of a hand-built skeleton as a `.glb` file for any glTF viewer.

## Limits

- Junctions are intersecting tubes, not one surface. A thick branch leaving at a shallow
  angle shows a crease where the two tubes meet.
- Where an axis bends more tightly than its own radius, rings overlap on the inside of
  the bend.
- Twigs are baked into the leaf part, one copy per site: a file does not shrink for
  repeating them. Flowers and fruits are not instanced yet, and leaf textures are not
  generated.
- Twig stems are labelled as foliage, since they belong to the leaf part.
