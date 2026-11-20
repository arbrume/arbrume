# Brush: painting where a plant grows

Strokes painted in space steer the self-organizing mode ([Growth](growth.md)): an artist
paints where the crown should be, draws the line a main limb or the trunk should follow,
erases growth, and freezes what has grown so far so that later strokes only add to it.
The plant grows again with the strokes, so its architecture stays that of its species:
it is the brush of TreeSketch (Longay et al. 2012) on the growth cycle of this crate.

The strokes are data, `arbrume_grow::Guides`, given to the generator next to the
surroundings: in Rust (`grow_guided`, `GenParams::with_guides`), in Python (the `guides`
keyword), on the command line (`arbrume gen --guides FILE`) and in Blender (the
*Arbrume Brush* tool, which stores them on the plant). A parametric species ignores them.

## Guides

A guides value is JSON (or the same structure in Python), in meters in the frame of the
plant: the plant stands at the origin, Y up, as for the [surroundings](growth.md#surroundings).

```json
{
  "strokes": [
    {"kind": "crown", "points": [[5.0, 8.0, 0.0], [8.0, 8.5, 0.0]], "radius": 1.5,
     "strength": 2.0, "reach": 6.0},
    {"kind": "limb", "points": [[0.0, 3.0, 0.0], [2.0, 4.0, 0.8], [5.0, 5.5, 0.0]],
     "radius": 0.4, "follow": 0.9},
    {"kind": "erase", "points": [[-3.0, 5.0, 0.0], [-3.0, 10.0, 0.0]], "radius": 1.2,
     "cycle": 16}
  ],
  "freezes": [16]
}
```

| Field | Default | Meaning |
|---|---|---|
| `kind` | | `crown`, `limb` or `erase` |
| `points` | | the stroke, in the order it was drawn; a limb needs two distinct points |
| `radius` | | radius of the brush, in meters |
| `density` | 1 | markers per cubic internode unit, as a multiple of the `marker_density` of the species (crown; limb, where 0 gives no markers) |
| `strength` | 1 | exposure given to the buds the stroke serves (crown, limb; see below) |
| `follow` | 1 | weight in `[0, 1]` of the stroke direction against the heading of the species (limb) |
| `reach` | 0 | distance in meters from which buds perceive the markers of the stroke; 0 for the `perception_distance` of the species (crown, limb) |
| `cycle` | 0 | first growth cycle in which the stroke acts |

`freezes` lists, in increasing order, the cycles at which the plant was frozen. The
defaults are starting points of this implementation, not measurements.

### Crown strokes

The volume of a crown stroke is the union of the balls of `radius` centred on the
stroke resampled every half radius. It is filled with markers of free space, uniformly,
`density` times the marker density of the species: the brush of Longay et al. (2012),
which scatters markers in a sphere moved along the stroke, after the space colonization
of Runions et al. (2007). Marker `i` of ball `b` is drawn from a key of the plant seed,
the index of the stroke and `b`, and kept when it is in its ball and in none of the
balls before it, so the union is filled once and the markers do not depend on anything
else.

Each cycle every live bud perceives these markers with the occupancy zone and the
perception cone of the species ([Space: markers](growth.md#space-markers)), the cone being
`reach` long. A bud that owns some markers gets the exposure `Q = strength * C` if it had
less (`C` is `full_exposure`), prefers the direction `V` towards its markers (Runions et
al. 2007, eq. 2), and the full-grown length of its axis may grow by one perception
distance past its `axis_length` while it sees them. This works with every `sensing`
mode: with `shadow` the markers of the strokes are the only markers.

The reach matters most. A stroke painted beyond the crown is only seen by buds within
`reach` of it: with the perception distance of `open_grown_broadleaf` (1.2 m), a stroke
painted 0.5 m past its crown at age 16 drew almost nothing, while with a reach of 6 m the
crown of the same plant at age 20 reached 2.4 m further into the stroke and its nodes
filled 78 % of the markers (`tests/guides.rs` in `arbrume-gen`). A plant grows at most
`shoot_length` per cycle along an axis, so a stroke far from the plant needs cycles to be
reached.

### Limb strokes

A limb stroke is taken by one axis, as in the selective growth of Longay et al. (2012):

- a stem, when the stroke starts within `radius` of the ground (`y <= radius`), else
- the lateral bud that can still grow and is closest to the start of the stroke within
  `radius` of it, the smallest meristem id on a tie. While no bud qualifies, the stroke
  waits: a lateral bud may form there later. Start a limb on the trunk or on a branch.

The axis gets `strength` times the light gathered by the whole plant (at least `C`) as
its exposure, so that at every branching point below it its share of the resource is
about that of the rest of the plant; its full-grown length becomes its length plus what
is left of the stroke, so it stops at the end of the stroke. Each metamer of the axis is
turned towards the point of the stroke one *lookahead* (two metamer lengths) further than
the point closest to its tip, searched forward from the progress already made, and
blended with the heading of the species:

$$
H' = \frac{(1 - f)\,H_s + f\,\frac{P(s^* + \ell) - p}{\lVert P(s^* + \ell) - p \rVert}}
{\lVert \ldots \rVert}
$$

with $H_s$ the heading of the species ([Orientation](growth.md#parametric-mode)), $f$ the
`follow`, $P$ the stroke by arc length, $s^*$ the arc length of the point closest to the
tip $p$ and $\ell$ the lookahead. This is a guiding direction field around the stroke, in
the spirit of the guiding vectors of Xu and Mould (2015), which bend least-cost paths
through a graph by making edges aligned with a vector field cheap; here the field acts on
the heading equation of Longay et al. (2012, eq. 2). The lookahead is a choice of this
implementation: the axis converges onto the stroke within a few metamers and cuts its
bends by about a lookahead. When the apex of a sympodial axis dies, its relay takes the
stroke over; when the axis is done, the stroke ends.

With `density > 0` the tube of `radius` around the stroke also holds crown markers, so
the limb bears leafy shoots.

Measured on `open_grown_broadleaf` at age 20 (`tests/guides.rs`): an S curve 7.1 m long
drawn from the trunk was followed along its whole length with a mean distance of 0.05 m
and a largest distance of 0.30 m from the nodes of the axis to the stroke; an S-shaped
trunk drawn from the ground, 9.5 m long, with 0.04 m and 0.29 m. The tests require a
mean below 0.15 m (0.1 m for the trunk), a largest distance below 0.6 m (0.3 m), and 90 %
of the stroke followed.

### Erase strokes

The volume of an erase stroke, the union of balls of `radius` along it as for a crown
stroke, blocks growth from the stroke's `cycle` on like a blocking occluder of the
surroundings ([Blocking](growth.md#surroundings)): the fate stage allots no metamer that
would enter it and the orientation stage ends a shoot at its edge. The markers of the
guides inside it are dropped. At its cycle, when that is after the first, every branch
with a live node inside it is shed with all it bears, from the first node of its axis
(the pruning of Longay et al. 2012, which removes a branch at its branching point). Stems
are never shed, and a stem that meets the volume stops there. The waist of the union of
balls between two centres is 0.968 times the radius.

### Freezing

From a freeze at cycle `F` on, only the buds that own markers of strokes acting from `F`
or later, and the axes of limb strokes acting from `F` or later, receive light; nothing
is shed by exposure (erase strokes still prune). The stages never look at the age of the
run, so the first `F` cycles of the longer run are those of the plant grown to `F`: the
positions, births and orders of the nodes born before `F` are equal to the bit, and the
plant only gains growth towards the new strokes. Radii follow the pipe model of the whole
plant, so an old axis thickens under what it now bears, and bending under weight, when
the species asks for it, carries the new weight. A freeze is undone by removing it and
the strokes acting from it; several freezes stack.

### Growth state

The guides are the growth state that is saved. A plant with guides is grown again from
its species, parameters, seed and guides, which takes milliseconds (below), and the guides
of a plant with a few strokes are about a kilobyte of JSON (1 122 bytes for one crown, one
limb and one erase stroke and a freeze). A snapshot of the engine itself would hold the
plant, the markers left and the shadow grid (3.9 MB of voxels for a tree of 20 000
nodes, [Performance](growth.md#performance-and-engine-options)), and would depend on the
internal layout of every stage: a new version could not read it, nor grow it on to the
same plant. Growing again needs only the version that grew the plant, which every
generated plant records in its metadata.

## Determinism and unchanged output

Guided growth depends only on the species, the parameters, the surroundings, the guides,
the seed and the version, never on threads: markers are given to buds as in
[Markers](growth.md#space-markers), each group of markers only to the buds within reach of
its bounding box, in their order. Without strokes and freezes `grow_guided` calls
`grow_in` and `GenParams` without guides takes the path it always took, so every plant
generated before is unchanged to the bit, which the snapshot tests check
(`snapshot_marker_grid`, `snapshot_occluder`, `snapshot_stands` and the Python
`test_snapshot_surroundings.py`). The metadata of an exported plant gains a `guides` key
only when the plant has some.

Guides are not applied to the plants of a stand (`generate_group`, `generate_stand`).

## Time

Growth of `open_grown_broadleaf`, seed 3, median of 7, release build, on the machine of
[Performance](growth.md#performance-and-engine-options):

| age | nodes | no guides | crown (reach 4 m) | limb | erase | the three |
|---|---|---|---|---|---|---|
| 20 | 4 777 | 3.7 ms | 8.8 ms | 5.8 ms | 5.5 ms | 10.8 ms |
| 30 | 16 706 | 12.8 ms | 56.7 ms | 18.2 ms | 15.8 ms | 24.0 ms |

Measured with no other build running; the times vary by tens of percent from run to run.
A crown stroke costs most for a long reach on a large plant: the markers are given to
every bud within reach of their box, cycle after cycle. In Blender, growth is a small part
of a regeneration; meshing and the upload of the mesh dominate (see
[Blender](blender.md#brush)).

## API

```rust
use arbrume_grow::{Guides, Stroke, StrokeKind, grow_guided};

let guides = Guides {
    strokes: vec![Stroke::new(StrokeKind::Limb, points, 0.4)],
    freezes: Vec::new(),
};
let (graph, report) = grow_guided(&species, &params, &surroundings, &guides);
assert!(report.strokes[0].claimed);
```

`grow_guided_timeline` keeps every earlier state for growth frames, and
`arbrume_gen::GenParams::with_guides` passes guides through the generation pipeline;
`Plant::guides` holds the report. Each `StrokeReport` gives:

| Field | Meaning |
|---|---|
| `markers` | markers scattered for the stroke |
| `reached` | those within the occupancy radius of a live node of the grown plant |
| `claimed` | whether an axis took the limb stroke |
| `followed`, `length` | arc length of the limb stroke along which its axis grew, and its length |
| `mean_distance`, `max_distance` | distance to the stroke of the live nodes the axis built while following it, before bending under weight |
| `pruned` | nodes shed by the erase stroke |

## References

- Longay, Runions, Boudon and Prusinkiewicz 2012, TreeSketch: interactive procedural
  modeling of trees on a tablet, Eurographics SBIM.
- Runions, Lane and Prusinkiewicz 2007, Modeling trees with a space colonization
  algorithm, Eurographics Workshop on Natural Phenomena.
- Xu and Mould 2015, Procedural tree modeling with guiding vectors, Computer Graphics
  Forum (Pacific Graphics).
