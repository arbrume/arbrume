# Climbing plants

Ivy, vines and creepers grow in the same engine and on the same plant graph as trees. A
species with a `[climbing]` table keeps its growth mode, its stages of light, resource
and shedding, its phyllotaxis and its foliage; only the orientation of its shoots changes,
and a last pass dresses the grown plant. The supports are the blocking volumes of the
[surroundings](growth.md#surroundings), so everything that can give a plant an obstacle
(a TOML file, Python dicts, voxel masks of Blender meshes) can give a climber a wall, a
post or a pergola.

```rust
use arbrume_grow::{GrowParams, Occluder, Shape, Surroundings, grow_in};
use glam::Vec3;

let ivy = arbrume_gen::preset("ivy_like").unwrap();
let wall = Shape::Box { min: Vec3::new(-3.0, 0.0, -0.42), max: Vec3::new(3.0, 4.0, -0.12) };
let surroundings = Surroundings { envelope: None, occluders: vec![Occluder::obstacle(wall)] };
let graph = grow_in(&ivy, &GrowParams { seed: 4, age: 16, scale: 1.0 }, &surroundings);
```

The model takes the local rules of Hädrich et al. (2017): a shoot near a surface turns
into it (their surface adaptation, eq. 6), turns towards light (their phototropism,
eq. 7), and is anchored to a surface it touches. Their particle simulation (shape
matching, anchor release, breaking, wind coupling) is not reproduced: a support holds a
stem for good, and a stem without support hangs because its orientation droops, not
because a dynamic simulation sags it.

## Supports

`arbrume_grow::climbing::Supports` collects, for one plant:

- every occluder of the surroundings with `blocks = true`, analytic (`box`, `cylinder`,
  `sphere`, `ellipsoid`, `cone`) or a voxel mask of an arbitrary mesh;
- the ground the plant stands on, when `climbing.ground` is true.

A support answers two questions at a point: the distance to its surface and the outward
normal there, the gradient of that distance by central differences. For a voxel mask the
distance is read from its exact distance field ([Growth](growth.md#surroundings)), whose
values are distances to the centres of the filled voxels, so half a voxel is taken off:
the surface of a mask lies on the faces of its voxels. A support whose bounding box is
farther than the reach of a query is skipped without evaluating its distance, so a scene
of many supports costs the ones near the shoot.

Whether a support is thin enough for a tendril or a twining stem is tested at the point:
the support is thinner than `t` there when the point `t` behind its surface, along the
normal, is outside it again. A post, a wire or a pergola beam is thin; a wall is not.

Another plant is a support when it is given as a voxel mask (in Blender, select its mesh
as a support). Plants of a stand grown together do not climb one another.

## Growth

`arbrume_grow::climbing::Climb` wraps the orientation stage of the growth mode
(`TropismBlend`), which says where a free shoot would go: the tropism of the species, the
direction `V` preferred by the environment and the random wander and gnarl. For each new
metamer it first finds what holds the tip:

1. **Held.** A support within `reach` of the tip (plus the stem offset) holds it. The
   wanted heading is projected on the tangent plane of the surface, so the tropism and the
   light act within the surface: `gravitropic_setpoint` 0 climbs straight up a wall, 90
   spreads level. The end of the metamer is then moved along the normal to the stem
   offset (the `gap` plus the tip radius), by at most a third of its distance to the
   surface per metamer, so that the straight continuation the fate stage checks never
   enters the support. The stem thereby follows the surface at a fixed distance, wraps
   over the top of a wall, around a post and into the reveal of a window.
   A twining stem instead takes the helix direction at `twine_angle_deg` from the upward
   direction on the surface; the sign of the angle sets the hand of the helix.
2. **Searching.** A support within `search_distance` turns the heading towards it by
   `seek_weight`. A shoot held by the ground creeps along it while it turns towards a
   support it could climb, so ivy planted a little away from a wall reaches the wall.
3. **Free.** Nothing holds the shoot. It takes a quarter of the turn the wrapped stage
   asks for, since a stem without support cannot carry its own tropism, and bends towards
   the ground by `droop` degrees per meter grown, so long free shoots arch over and hang.
   The quarter is a choice of this implementation, not a measurement.

The adhesive habit holds any support; the tendril and twining habits hold only supports
thinner than `max_support_thickness` at the tip, and the ground.

No metamer ends below the ground, inside a blocking occluder or out of the envelope. A
metamer that would is turned along the surface it runs into, so a shoot slides along a
wall it cannot hold; then it is tried straight ahead; when both are blocked the shoot
ends, its apex alive, as with `Confined`. A heading whose next metamer would be blocked is
turned along the obstacle as well, since the fate stage holds back a bud whose straight
continuation is blocked.

In the self-organizing mode the shoots of a climber compete for light like those of a
tree: every metamer casts its shadow down the support, so the resource flows to the buds
that reach lit, free surface, and the plant spreads over the support instead of piling up.
The occluders of the surroundings cast their shade as for a tree; a support given
`shade = 0` (the support role of Blender) casts none. In the parametric mode the lengths
come from the gradient curves and only the orientation climbs.

## Dressing

After the radii are set, `arbrume_grow::climbing::dress` runs three passes:

1. **Hugging.** A held node whose bark would come closer to its support than `gap`, because
   its stem thickened after growth placed it, is moved out along the normal. No stem enters
   its support nor lies on its surface, so the bark does not z-fight with the wall.
2. **Leaves.** The leaf sites of held nodes turn, with the weight `leaf_facing`, towards a
   frame whose upper side faces away from the surface and a little up and whose blade
   points along the surface and out of it: the leaves of ivy lie like tiles over the wall.
3. **Attachment organs**, appended to the graph as short axes. The adhesive habit gets
   rootlets from the bark of held nodes to the surface, with probability `rootlets` per
   node; the tendril habit gets tendrils from nodes near a thin support, with probability
   `tendrils`: a stalk that leaves along the surface and a little out of it, then a coil
   of `tendril_turns` turns. An organ that would enter its support or the ground is left out. Organs are
   at most half as thick as the tip radius; they are labelled `branch` like any lateral
   axis in the annotations, and carry the birth of their node, so growth frames show them
   with it.

## Parameters of `[climbing]`

Lengths are in meters and are not multiplied by the scale of the plant: they describe the
attachment organs. The defaults are starting points for tuning, not measurements.

| Field | Default | Meaning |
|---|---|---|
| `attachment` | `adhesive` | `adhesive` (any surface), `tendril` or `twining` (thin supports only) |
| `reach` | 0.06 | distance from a surface within which a shoot is held |
| `search_distance` | 0.5 | distance within which a shoot turns towards a support; at least `reach` |
| `seek_weight` | 0.4 | weight in `[0, 1]` of the turn towards a support |
| `gap` | 0.004 | gap between the bark of a held stem and its support; at most `reach` |
| `droop` | 90 | bend of a free shoot towards the ground, degrees per meter |
| `ground` | true | the ground is a support: shoots that reach it creep |
| `max_support_thickness` | 0.3 | thickest support a tendril or twining stem holds |
| `twine_angle_deg` | 55 | helix angle of a twining stem from the upward direction, in `[-85, 85]` |
| `tendrils` | 0.5 | probability per node near a thin support of a tendril |
| `tendril_length` | 0.12 | length of a tendril |
| `tendril_turns` | 4 | turns of the coil of a tendril |
| `rootlets` | 0 | probability per held node of a rootlet (adhesive habit) |
| `rootlet_length` | 0.012 | length of a rootlet |
| `leaf_facing` | 0.8 | weight in `[0, 1]` with which the leaves of held nodes face away from the support |

## Presets

| Preset | Habit | What it does |
|---|---|---|
| `ivy_like` | adhesive, rootlets | one stem climbs the nearest support and fans out over it; shoots wrap over the top of a wall and fill around a window; on open ground it creeps |
| `vine_like` | tendril | climbs a post or the posts of a pergola with coiled tendrils, runs along the beams and hangs between them; a wall is too thick for its tendrils, so it sprawls at its foot |
| `creeper_like` | adhesive, rootlets | four stems run out level over the ground and over rocks and steps, small leaves facing up; a wall stops it |

The three have `variation.weight = 0`: the random sampler (`arbrume gen --random`,
`RandomSampler`) never draws them, since a climber is only itself with a support, and the
random mix of earlier versions is unchanged. Generated without surroundings a climber
creeps on the ground as a ground cover, which is what the generic tests of every preset
see. Give a climber a support through the surroundings (below).

Look at them with the development sheet, which draws the blocking occluders
([Command line](cli.md#looking-at-species)):

```sh
arbrume sheet --species ivy_like --ages 10,16,22 --surroundings examples/climbing/window_wall.toml --out ivy.png
arbrume sheet --species vine_like --ages 6,10,14 --surroundings examples/climbing/pergola.toml --out vine.png
arbrume sheet --species creeper_like --ages 8,12 --surroundings examples/climbing/rocks.toml --out creeper.png
```

## Performance

Measured with `arbrume gen --manifest` (release build, one plant, default engine options,
median of three runs) on the 16-thread desktop of [Growth](growth.md), while other builds
were running on it; the scenes are those of the sheets above, the nodes include those of
the rootlets and tendrils:

| Plant | Nodes | Leaf sites | Triangles | Grow | Mesh |
|---|---|---|---|---|---|
| `ivy_like`, age 22, wall 6 m by 4 m with a window | 19 184 | 14 642 | 99 636 | 25.7 ms | 20.6 ms |
| `ivy_like`, age 16, same wall | 10 855 | 7 599 | 71 843 | 11.3 ms | 20.0 ms |
| `vine_like`, age 10, pergola | 5 747 | 1 078 | 45 027 | 3.1 ms | 17.2 ms |
| `creeper_like`, age 12, four rocks | 11 382 | 16 173 | 93 159 | 7.6 ms | 7.4 ms |

The climbing stage costs a few distance evaluations per metamer and support near it (the
distance and its six-sample gradient at the tip, the same at the end of the metamer). It
is part of the single-threaded structure pass, like the rest of the orientation, so a
climber is bitwise identical for every thread count.

## Limits

- A support holds a stem for good: there is no anchor release, no breaking and no
  sagging of a held stem under its weight. Weight acts only through the droop of free
  shoots, and `weight_bending` should stay 0 for climbers, since that pass bends held
  stems away from their support.
- Hanging comes from the droop of the orientation stage. A shoot that grows over a convex
  edge is held around it, so ivy wraps over the top of a wall rather than hanging from it
  in curtains; long free laterals do hang.
- Plants of a stand (`grow_together`) grow without the climbing stage, and climbers do not
  climb other plants unless these are given as voxel masks.
- Supports are read through their distance function. A voxel mask approximates a slanted
  surface by steps of one voxel; a stem follows the steps within the gap. Voxels of a few
  centimeters suit a wall; a post or a wire needs voxels finer than its radius, or an
  analytic cylinder.
- Tendrils coil in the air next to their support, not around it.

## References

- Hädrich, Benes, Deussen and Pirk 2017, Interactive modeling and authoring of climbing
  plants, Computer Graphics Forum 36(2).
- Palubicki et al. 2009, Self-organizing tree models for image synthesis, ACM TOG.
- Runions, Lane and Prusinkiewicz 2007, Modeling trees with a space colonization
  algorithm, Eurographics Workshop on Natural Phenomena.
