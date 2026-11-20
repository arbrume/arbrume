# Junctions

Where a lateral axis leaves its parent, the mesher offers two constructions, chosen with
`MeshOptions::junctions` (`--junctions` on the command line, `junctions=` in Python, the
*Junctions* property in Blender):

| | Tier A (default) | Tier B |
|---|---|---|
| construction | the child tube starts on the parent centerline, inside it, widened into a collar where it comes out | the child is stitched into a hole cut in the parent surface |
| bark surface | one closed tube per axis, open at the base of each child; tubes intersect | one closed surface per stem, once vertices of equal position are welded |
| use | rendering, games, datasets | 3D printing, subdivision surfaces, booleans, anything that needs a solid |
| cost (bark and leaves) | 1 | 1.5 to 2.2 times Tier A (see below) |

Tier A stays the default: at viewing distance the two look the same (same collar, same
shading), and Tier B costs more time and triangles. This page describes Tier B, how it
was chosen, and what it guarantees.

## Tier B: stitched junctions

Everything is planned from the graph before any buffer is filled (`crate::junction` in
`arbrume-mesh`), so `mesh_counts` still gives the exact sizes in advance and the fill
stays one independent task per chain, bit-identical whatever the number of threads.

**Footprint.** Near its base the child is a surface of revolution about the line leaving
the attachment point along its first internode, of radius `rho(s)`: its base radius
widened by the collar of the species (as in Tier A). Near the attachment point the parent
is a cylinder about its tangent there, whose radius varies linearly along it. For each of
the `n` generators of the child (one per side), the arc length `s` at which it leaves the
parent is the larger root of a quadratic, `s^2 + rho^2 - h^2 = (R + k h)^2` with
`h = a s + b` the height along the parent; `rho` depends on `s`, so the root is solved
again with the radius at the previous root (four times at most). The `n` points form the
footprint, a closed loop on the parent where the child meets it.

**Collar.** The child starts at its footprint: a few skewed rings go from the footprint,
each vertex at its own arc length, to the first round ring of the child, placed a margin
beyond the farthest footprint point. Along each generator they follow `rho`, so the
widening of the Tier A collar is kept. The part of the child inside its parent is left
out. The normals of the collar are those of the collar as meshed (cross product of the
chords to the neighbours around and along), so they agree with its skewed quads.

**Hole.** The parent gets a ring at each end of the footprint, a margin beyond it along
the parent; all its rings between those two get the same number of sides, so the surface
there is a grid of quads, and the quads whose columns meet the footprint (widened by the
same margin) are left out. A rectangle of grid cells always has one simple loop as its
border, the rim. The margin is half the base radius of the child, and at least the
sagitta `R (1 - cos(pi / n))` of the sides of the parent: the most its faceted surface
departs from the smooth one, which the footprint lies on.

**Stitch.** The rim and the footprint are two nested loops on the parent. They are
joined by triangles in the order of their angles about the centre of the footprint, in
the plane unrolled from the parent (`x` around it, `y` along it), stretched along `y` so
that the bounding box of the footprint is square: the same zip as two rings of different
side counts, `m + n` triangles for a rim of `m` vertices and a footprint of `n`.

**Copies.** The rim and the footprint are written twice, as texture seams are: once in
the frame of the parent for the stitch (its texture coordinates, normals and colours),
once in the frame of the child for the collar. The copies have exactly the same
positions, computed by the same code, so the surface closes when vertices of equal
position are welded. In Blender, *Merge by Distance* with distance 0 does that and keeps
the texture seams, which are stored per face corner.

**Conflicts.** Holes of one parent never share a cell. When two would (whorls, or
laterals on neighbouring nodes), the rings under them get twice as many sides, up to 64,
at most three times; what still overlaps is trimmed from the later hole, and a footprint
point left outside its hole is moved into it, on the parent surface as meshed; the collar
fades that shift out towards its first round ring. A hole stays within the round rings
of its parent, after the collar of the parent itself. A child whose hole would pass the
tip of its parent prolongs the parent straight beyond its tip by what is needed, keeping
its tip radius. A child whose first round ring would lie beyond its own tip is entirely
inside its parent and is not meshed.

**Rings near stops.** On an axis that carries holes, the tangent of a ring placed on a
straight run of merged nodes is blended between those of the two ends of the run, so a
ring added at the end of a footprint does not tilt against the ring of the node next to
it. Tier A rings are unchanged.

### What rides on the surface

* **Labels** (`Mesher::provenance`). The collar, the footprint and the copies of the rim
  belong to the child: every vertex of the stitch and of the collar is labelled with the
  child axis, and every triangle has its three vertices in one axis. A rim copy has the
  position of a parent vertex and the label of the child.
* **Texture coordinates.** The parent keeps its mapping up to the rim; the stitch
  continues it (`u` unwrapped across the seam of the parent when the hole straddles it,
  which differs from the parent by whole repeats only). The collar continues the mapping
  of the child: `v` by arc length per vertex, `u` around. The footprint is therefore a
  texture seam between the two mappings, as the border of the parent and child tubes is
  in Tier A.
* **Wind** ([Wind](wind.md)). The rim copies carry the wind data of the parent vertices
  they copy, and the footprint (both copies) the data of the parent where it lies, so the
  stitch moves with the parent; the collar rings beyond carry the data of the child.
  Coincident vertices always have the same data, so nothing tears. The data of a child
  matches that of its parent at its base, except for a child of a branch of the deepest
  wind level (3): it pivots on its own base and leaves out the bending of its parent at
  that level, which its collar then absorbs as stretch (in Tier A, as a gap). On the
  synthetic `medium` skeleton (seed 4) under that shader at four times, the edges across
  junctions change length by at most 1.9 mm, the other edges by at most 5.1 mm, and the
  collars of children of the deepest level by at most 6.1 mm.
* **Levels of detail.** The construction is the same at any tolerance: fewer sides and
  rings give fewer triangles, and a coarser level never has more triangles than a finer
  one (tested from quality 1 down to 0.25).
* **Twigs and leaves** are untouched: twig templates are instanced in the leaf part and
  their stems stay as in Tier A.

## Guarantees and checks

`arbrume_mesh::topology::check_surface` welds equal positions and reports boundary,
non-manifold and misoriented edges, non-manifold vertices, components, sharp creases and
self-intersections (a bounding volume hierarchy and the interval test of Möller 1997 in
double precision). A sharp crease is reported, not counted against watertightness: two
triangles at less than about 26 degrees from each other, which is what the crotch of a
narrow fork looks like.

On every preset at ages 5 and 12 (seed 1, full quality, 54 meshes), the Tier B bark is a
closed manifold (no boundary, non-manifold or misoriented edge, no non-manifold vertex,
no degenerate triangle) with one component per stem. A multi-stemmed plant (shrubs,
bamboo, fern, rosette) has one closed surface per stem; the stems are not joined to each
other.

Self-intersections remain where axes cross each other in the grown skeleton, which no
junction construction can remove, and at some junctions: 6 of the 54 meshes have pairs of
intersecting triangles between axes that meet at a junction (77 pairs in all):
`hedge_shrub` 12 (33), `poplar_like` 12 (17), `spreading_shrub` 5 (12) and 12 (4),
`thicket_shrub` 12 (8), `palm_like` 12 (3). They come from wide footprints (a child almost
as thick as its parent) and from children attached right at the collar of their own
parent, where the hole cannot reach. The trees `broadleaf`, `oak_like`, `pine_like` and
`open_grown_broadleaf` have none, which the tests assert.

The tests (`crates/arbrume-mesh/tests/junctions.rs`, `crates/arbrume-gen/tests/junctions.rs`)
check: watertightness on hand-built cases (single laterals from 0.1 mm to 8 cm on trunks
of 1 and 10 cm, whorls, stacked laterals, a fork, laterals at the tip and at the first
node) and on every preset at two ages; determinism with and without the thread pool and
with a reused mesher; counts known in advance; labels and texture coordinates of the copies;
wind continuity (coincident vertices move together under the demonstration shader of the
wind page); fewer triangles at lower levels of detail. Files written with Tier B pass the
Khronos glTF validator with no error or warning, with wind data, levels of detail and
textures.

## Cost

`arbrume bench --junctions` (plant 0 of base seed 1, quality 1, median of 15 meshings with
a warm mesher, bark and leaves):

| species | age | bark triangles A | bark triangles B | mesh ms A | mesh ms B | B / A |
|---|---|---|---|---|---|---|
| broadleaf | 12 | 5223 | 8520 | 0.175 | 0.278 | 1.59 |
| broadleaf | 20 | 19310 | 26480 | 0.486 | 0.870 | 1.79 |
| oak_like | 12 | 2180 | 2472 | 0.044 | 0.067 | 1.50 |
| oak_like | 20 | 8064 | 9304 | 0.125 | 0.222 | 1.77 |
| pine_like | 12 | 10597 | 15906 | 0.241 | 0.523 | 2.17 |
| pine_like | 20 | 55668 | 69602 | 0.964 | 1.881 | 1.95 |
| open_grown_broadleaf | 12 | 17892 | 21346 | 0.338 | 0.643 | 1.90 |
| open_grown_broadleaf | 20 | 90987 | 109340 | 2.180 | 3.433 | 1.58 |
| shrub | 12 | 20005 | 27030 | 0.730 | 1.257 | 1.72 |
| shrub | 20 | 20041 | 27060 | 0.736 | 1.183 | 1.61 |

The bark alone costs 1.8 to 3.5 times Tier A, most of it in planning the holes (which is
sequential), and has 13 to 63 percent more triangles at full quality, about twice as many
at quality 0.5: the rings added at the ends of each footprint, the rings refined under
whorls, the stitches and the collars. The criterion suite (`cargo bench -p arbrume-mesh`,
group `junctions`) compares the two on synthetic skeletons, bark only: 2.3 ms against
7.6 ms for `medium` (10 503 nodes, 109 116 Tier A triangles) and 18.5 ms against 56.5 ms
for `large` (86 615 nodes, 837 584), measured on a loaded machine. Growing a plant costs far more than meshing it, so the
difference rarely shows in a batch.

## How it was chosen

Two methods were implemented and measured on the same five plants (seed 1, quality 1:
`broadleaf` age 10, `oak_like` 12, `pine_like` 12, `open_grown_broadleaf` 12, `shrub` 6):

* **Scaffold** (Fuentes Suárez and Hubert 2018): every node is a joint; its cells are the
  spherical Voronoi regions of its incident directions (from the convex hull of those
  directions: arcs between the two poles for a planar hull, between the normals of
  adjacent faces otherwise); the subdivision counts of the arcs, which the paper finds
  with an integer program, are propagated from the root instead, which is possible on a
  tree since each joint receives the count of its incoming segment; the cells of the two
  ends of each internode are linked by the cyclic shift of least length; the cell points
  are projected from the joint onto the union of the incident cylinders.
* **Stitch**, the method above.

Strand-based meshing (Li et al. 2024) was not built: the paper reports seconds per tree,
two orders of magnitude above the budget of the mesher. A quad layout with one global
integer program (Usai et al. 2015) would need a solver the project cannot ship, and with
a propagated assignment it becomes the scaffold above.

| plant | method | bark ms | triangles | open edges | components | intersecting pairs | median quality | slivers |
|---|---|---|---|---|---|---|---|---|
| broadleaf | Tier A | 0.051 | 2983 | 231 | 56 | 698 | 0.13 | 31% |
| | scaffold | 0.41 | 2188 | 0 | 1 | 3 | 0.05 | 71% |
| | stitch | 0.121 | 4602 | 0 | 1 | 0 | 0.29 | 20% |
| oak_like | Tier A | 0.030 | 1454 | 84 | 16 | 342 | 0.13 | 23% |
| | scaffold | 0.08 | 798 | 0 | 1 | 0 | 0.06 | 75% |
| | stitch | 0.054 | 1896 | 0 | 1 | 0 | 0.46 | 15% |
| pine_like | Tier A | 0.132 | 9000 | 592 | 137 | 2528 | 0.16 | 29% |
| | scaffold | 0.81 | 5352 | 0 | 1 | 22 | 0.07 | 57% |
| | stitch | 0.374 | 14996 | 0 | 1 | 0 | 0.26 | 21% |
| open_grown_broadleaf | Tier A | 0.177 | 13495 | 975 | 172 | 4180 | 0.14 | 30% |
| | scaffold | 0.83 | 6658 | 0 | 1 | 8 | 0.04 | 80% |
| | stitch | 0.467 | 18190 | 0 | 1 | 0 | 0.37 | 20% |
| shrub | Tier A | 0.168 | 9107 | 729 | 217 | 2448 | 0.12 | 38% |
| | scaffold | 1.21 | 11264 | 0 | 9 | 146 | 0.08 | 60% |
| | stitch | 0.427 | 14872 | 0 | 9 | 127 | 0.26 | 23% |

Quality is `4 sqrt(3) area / (sum of squared edge lengths)` per triangle, 1 for an
equilateral triangle; slivers are triangles below 0.1. The scaffold times are those of a
prototype that allocates per joint, not of an optimized implementation; the stitch
times are bark only, with a warm mesher. The shrub has nine stems; its remaining
intersections are between stems and branches that cross in the skeleton.

The scaffold closes the surface at little cost, but:

* its cells ignore radii: the cell of a thin child takes as much of the joint sphere as
  that of the trunk, so whorls become webbed blobs and the collar disappears;
* its resolution is one quad band per internode, not the tolerance of the mesher: curved
  internodes are cut by chords, and the side counts are those of the propagated cells
  rather than of the radius;
* the propagated counts fail where a joint has a non-planar hull with more arcs around
  the incoming region than that region has points (42 joints of the pine), which leaves
  folded triangles;
* almost every triangle is a long sliver.

Weighting the cells by radius would turn it into a different method, close to the
stitch. The stitch keeps the rings, the tolerance, the collar and the texture mapping of
Tier A, changes them only around each footprint, and is the only one of the two whose
junctions do not intersect on the four trees. It was chosen.

## Limits

* Children almost as thick as their parent, at a shallow angle, have wide footprints;
  their stitch triangles span a large angle around the parent and can cut the collar
  (the 77 pairs above).
* A child attached right next to the collar of its own parent cannot open its hole
  there: its footprint is pressed against the end of the round rings.
* On a parent of very few sides (3 to 5, at coarse tolerances) a stitch triangle can lean
  away from the normal of a rim corner, which keeps the normal of the parent so that the
  shading is continuous.
* The grid vertices inside a hole stay in the buffers, unused.
* Refining the rings under a whorl adds long triangles where the refined rings are zipped
  to their neighbours.
* The level cap of the wind data (above) stretches the collars of the deepest branches.
* Stems of a multi-stemmed plant are separate closed surfaces.
