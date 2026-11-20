# Underwater plants

Kelp, seagrass, waterweed, tape grass and water lilies grow in the same engine and on the
same plant graph as trees. A species with an `[aquatic]` table keeps its growth mode,
its stages of light, resource and shedding, its phyllotaxis and its foliage; buoyancy
takes the place of gravitropism below the water surface, light fades with depth, and
passes on the grown plant bend it with the current and float its canopy, its floats and
its pads on the surface. A species without the table grows exactly as before, with or
without water around it.

```rust
use arbrume_gen::{GenParams, Surroundings, Water, generate, preset};
use glam::Vec3;

let kelp = preset("kelp_like").unwrap();
let water = Water { current: Vec3::new(0.3, 0.0, 0.0), ..Water::still(8.0) };
let surroundings = Surroundings { water: Some(water), ..Surroundings::none() };
let plant = generate(&kelp, &GenParams::new(3, 14).with_surroundings(surroundings))?;
```

## What the model rests on

Arbrume had no paper on water plants in its reference library; the botany and the
physics below come from the sources listed at the end, read on 2026-11-20. Numbers in
this page are of three kinds, always said which: measurements reported by a source,
standard physics (drag law, buoyancy, Beer-Lambert), and starting values of this
implementation chosen by eye, which are not measurements.

**Giant kelp** (*Macrocystis pyrifera*). A holdfast of root-like haptera grips the rock
and does not take up nutrients like a root system; it is conical in plants a year old or
more. Fronds rise from a basal branching system of stipes; a frond is a stipe with its
blades, each blade with a hollow gas float (pneumatocyst) at its base, the lowest two or
three blades excepted. Fronds grow upwards and, once their apex reaches the surface, are
pushed out horizontally by continued growth to form the canopy. Mature fronds are
reported 5 to 10 m long or more, up to about 45 m, with 100 to 200 blades or more; plants
in exposed sites bear 40 to 60 fronds, sheltered ones several hundred (FAO, Kelp
resources; Wikipedia, *Macrocystis*).

**Bull kelp** (*Nereocystis luetkeana*). A single hollow stipe 10 to 36 m long ends in
one gas-filled float from which about 30 to 64 blades grow, up to 4 m long and 15 cm
wide (a later passage of the same article says up to 10 m); the holdfast is up to about
40 cm across with branched haptera (Wikipedia, *Nereocystis*). The size of the float is
not given by the sources read.

**Seagrass** (*Zostera marina*). A rhizome grows horizontally through the sediment and
anchors by clusters of roots at its nodes; for each new leaf a new rhizome segment
(internode) is formed. Ramets are built of modules (a piece of rhizome, leaves and
roots) and most ramets of a meadow are produced as the rhizome elongates. Leaves are up
to 1.2 cm wide and over 1 m long in deeper water, only a few centimeters in clear shallow
water; shoots carry three to seven leaves (Wikipedia, *Zostera marina*; the search
summaries of Duarte-type clonal studies cited below).

**Waterweed** (*Elodea canadensis*). Leaves usually three per whorl, oblong, 6 to 17 mm
long and 1 to 4 mm broad, bright green and translucent; stems grow indefinitely at their
tips, single specimens 3 m or more; adventitious roots form along the stem (Wikipedia,
*Elodea canadensis*). Its tendency to grow most near the surface is reproduced here by
light competition, not prescribed.

**Tape grass** (*Vallisneria*). Leaves arise in clusters, long ribbons with rounded tips
and raised veins, 30 to 60 cm in *V. spiralis*, over 1 m in giant forms; the plant
spreads by runners whose daughter plants form at their ends and nodes; the female
flowers rise to the surface on long uncoiling stalks (Wikipedia, *Vallisneria*).

**Water lily** (*Nymphaea alba*). Round dark green floating leaves up to 35 cm, a
rhizome in the mud with a flattened apex, water depths from about 0.2 m in cultivation
to 0.9 m and deeper in nature, a spread of about 1.5 m per plant, flowers 8 to 20 cm
across, white with yellow stamens (Wikipedia, *Nymphaea alba*). The notch of the pad is
common knowledge for the genus and is not quantified by the sources read.

**Light.** Downwelling irradiance falls exponentially below the surface,
`E(z) = E(0) exp(-Kd z)`, with `Kd` the diffuse attenuation coefficient (Lee, Shang and
Stavn 2018 define the terms; the form is the Beer-Lambert law). `Kd` is not constant
with depth for the whole PAR band and the law holds worst in turbid water (same
source). Lin et al. (2016, cited in the search summary) report a related band-averaged
coefficient from about 0.02 per meter in ocean gyres to about 5.2 per meter in very
turbid coastal water; a euphotic depth (1 % of surface light) of about 200 m in the open
ocean (Wikipedia, *Photic zone*) corresponds to `Kd` of about 0.023 per meter. A
mesocosm study on *Macrocystis* used `Kd` of 0.8, 1.2 and 1.6 per meter as treatments
(Celis-Plá et al. 2021, as summarized by the search), and kelp canopies can reduce the
light at the bottom to under 1 to 3 % of the surface (Reed and Foster 1984, as cited).
The default `attenuation` of 0.2 per meter is a starting value between clear coastal and
turbid water, not a measurement.

**Forces.** Hydrodynamic drag grows with the square of the velocity (Burnett and Koehl
2022); a flexible kelp avoids the brunt of it by "going with the flow", bending and
reconfiguring (Koehl and Wainwright 1977, as cited there). The drag of a cylinder across
a flow at high Reynolds number has a coefficient of order 1 (Wikipedia, *Drag
coefficient*). Buoyancy is Archimedes' principle. The Young's modulus of a kelp stipe was
not found in the sources reachable without a subscription, so `stiffness` is a starting
value, not a measurement.

## The water

The water is part of the [surroundings](growth.md#surroundings) of a plant,
`Surroundings::water`, in meters in the frame of the plant (Y up, the base at the
origin). In a surroundings file it is a `[water]` table:

```toml
[water]
surface = 8.0            # height of the surface above the base of the plant
current = [0.3, 0.0, 0.0]  # m/s; the vertical part is ignored
attenuation = 0.2        # Kd of light, per meter
density = 1025.0         # kg/m3: 1000 fresh water, about 1025 sea water
```

| Field | Default | Meaning |
|---|---|---|
| `surface` | the `aquatic.water_depth` of the species | height of the water surface above the base of the plant |
| `current` | 0 | velocity of a steady current |
| `attenuation` | 0.2 per meter | diffuse attenuation coefficient `Kd` |
| `density` | 1000 kg/m³ | density of the water |

Without a `[water]` table an aquatic species grows in still water as deep as its
`aquatic.water_depth`. A land species ignores the water: its plant is the same to the
bit (tested).

## Growth

`arbrume_grow::aquatic` holds the stages and passes. With `[aquatic]`, `grow_in`
(and every front end) calls `aquatic::grow_underwater`.

### Buoyancy and the surface

`Submerged` wraps the orientation stage of the growth mode. Below the surface the
heading `H` of the wrapped stage is blended with the upward direction,
`H' = (1 - b) H + b Y`, `b` the `buoyancy`: buoyant tissue rises whatever the
gravitropic set-point, so a set-point of 40 degrees with a small `b` gives a leaning
petiole and a large `b` an upright stipe. At the surface the habit decides:

- `spread`: an axis whose tip is within a quarter of an internode of the surface turns
  level and aims each metamer at the surface (with a slope of at most 30 degrees): the
  canopy of a giant kelp, the mat of a waterweed. Lengths are not changed, so a frond
  longer than the water is deep lays its excess on the surface;
- `stop`: an axis grows until its tip is within 1.5 cm of the surface, then ends its
  shoot, its apex alive: the petiole of a water lily, the stipe of a bull kelp.

### Light

`Attenuated` wraps the environment stage and multiplies the exposure `Q` of each bud by
`exp(-Kd (surface - y))`. In the self-organizing mode deeper buds then receive less of
the resource (Borchert-Honda or priority allocation) and shed their branches first, so a
plant spends its growth near the surface; in murky water a waterweed stays short (the
test `light_fades_with_depth_and_land_plants_ignore_water`: with `Kd` 2.5 per meter
the plant has fewer than half the nodes it has in clear water and stays under half the
depth). In the parametric mode exposures are not read.

### Forces of the water

After the radii, every internode carries the net buoyancy of its tissue,
`s rho g V` upwards (`s` the `stem_buoyancy`, `1 - rho_tissue / rho_water`), and the
drag of the current across it, `0.5 rho Cd (2 r l) |u_n| u_n` (`u_n` the part of the
current normal to the internode, `Cd` the `drag_coefficient`); the leaves of a node add
the drag of blades lying in the flow, `0.5 rho Cd_leaf A |u| u`, and their lift,
`leaf_lift A` plus the buoyancy of their floats. As in the bending under weight
([Growth](growth.md#bending-under-weight)) every internode is a cantilever: it turns
with all it carries by `|L x F| l / (E pi r^4 / 4)` about `L x F` (`F` the forces it
carries, `L` the lever to their point of application, `E` the `stiffness`), limited to
the angle that aligns the lever with the force. A limp stem therefore lines up with the
force it carries, like a tethered float, and leans downstream by the ratio of drag to
lift. Nodes at the surface are held up by it: the stems below them carry only the
horizontal part of their forces, and they only turn about the vertical, so a canopy
swings downstream and stays on the surface. The pass runs once, base to tips, in a
fixed order: deterministic, first order, not iterated to equilibrium.

### The surface, leaves, pads and floats

Then, in this order (`aquatic::dress`):

1. **Clamp.** Every node is brought under the surface by its radius; the nodes at the
   surface (within their radius and 3 cm below it, or above) are laid at that height.
   No stem crosses the surface; canopies lie on it.
2. **Pads** (`pads = true`). Every axis that reaches the surface ends in one leaf lying
   flat on it, 4 mm above it, and the other leaves are dropped. They are placed before
   the forces, which the dropped leaves would otherwise load.
3. **Tip floats** (`[aquatic.tip_float]`). Every stem ends in a float: a few metamers
   whose radii follow an ellipse up to `radius`; on a stem at the surface it lies on the
   surface downstream, its axis 0.7 of its radius under the water, so its top shows; on
   a younger stem it continues the stipe. The leaves of the stem are replaced by
   `blades` leaves at the far end of the float, fanned downstream on the surface or
   splayed all around under it.
4. **Leaves.** Leaves of nodes at the surface turn flat on it (`floating_leaves`), and
   trail downstream by their `leaf_flex`. Submerged leaves turn by `leaf_flex` towards
   the force of the water on them (lift up, drag downstream), and a leaf whose tip would
   cross the surface is tilted until its tip is under it, allowing for the random turn
   of `foliage.orientation_variation_deg` the mesher adds.
5. **Holdfast** (`[aquatic.holdfast]`). `haptera` claws arch from the first node of the
   stems down to the bottom, evenly around the base, each `reach` meters long and forking
   `forks` times near its end. They are lateral axes flagged as roots: labelled `root`
   in the annotations, still in the wind data, not bent by the water. The root system
   of `[roots]` grows into the soil, which haptera do not enter, so it is not used for
   them.

### Meadows: rhizomes and ramets

With `[aquatic.rhizome]` the plant is a clone: `axes` rhizomes leave its base and add
`extension` nodes `spacing` meters apart every cycle on the bottom, wandering by
`wander_deg`; a node starts a lateral rhizome with `branch_probability`, at
`branch_angle_deg`, up to `max_order`; growth stops at `max_ramets`. Every node bears a
ramet: the species without its rhizome, grown with its own seed for the cycles left
after its node appeared, so the ramets at the front of the clone are young. Every
aquatic plant, meadows included, is the same for any number of threads (tested with 1,
3 and 8). In the graph a rhizome leaving the base is a
stem, a lateral rhizome a lateral of its parent, sunk into the bottom by `burial` of
its radius; every stem of a ramet is a relay of the rhizome node that bears it. Ramets
are grown alone, one after the other on the calling thread (a meadow is one job of a
batch or of a forest library, which are the parallel units): they do not shade one
another nor see obstacles.

### Determinism and age

Everything above runs on the calling thread from keys derived from the seed. A growth
sequence (`--growth-frames`, `Generator::timeline`) of an
aquatic plant cuts the dressed plant at earlier cycles: the last frame is the plant of
`generate` to the bit, earlier frames keep the final positions and take the radii of
their age, and a meadow shows its younger ramets appear.

## Parameters of `[aquatic]`

| Field | Default | Meaning |
|---|---|---|
| `water_depth` | 2 m | height of the surface when the surroundings give none |
| `buoyancy` | 0.3 | weight `b` of the upward direction below the surface |
| `surface` | `spread` | `spread` or `stop` at the surface |
| `stiffness` | 5 MPa | Young's modulus `E` of the stems |
| `stem_buoyancy` | 0.05 | net lift of a stem per its volume, as a fraction of the weight of the water it displaces; negative sinks |
| `drag_coefficient` | 1 | `Cd` of a stem across the flow |
| `leaf_drag_coefficient` | 0.05 | `Cd_leaf` of a blade lying in the flow, per its area |
| `leaf_lift` | 1 N/m² | net lift of a blade per its area, floats excluded |
| `leaf_flex` | 0.6 | how far a submerged leaf turns towards the force on it, and a floating one downstream |
| `floating_leaves` | 1 | how flat the leaves at the surface lie on it |
| `pads` | false | axes reaching the surface end in one floating leaf |
| `blade_float` | 0 | length of the float at the base of every blade, in leaf lengths |
| `float_color` | `[0.35, 0.28, 0.08]` | colour of the floats |
| `holdfast` | none | `haptera` (10), `reach` (0.2 m), `radius` (8 mm), `forks` (2), `variation` (0.3) |
| `tip_float` | none | `radius` (6 cm), `length` (15 cm), `blades` (24) |
| `rhizome` | none | `axes` (2), `spacing` (0.12 m), `extension` (2), `branch_probability` (0.12), `branch_angle_deg` (55), `wander_deg` (10), `max_order` (2), `radius` (3 mm), `burial` (0.5), `max_ramets` (80) |

All defaults are starting values chosen by eye, not measurements. A species cannot both
climb and grow under water.

## Geometry

**Strap and ribbon leaves.** `foliage.shape = "strap"` is a long blade of nearly
constant width with a rounded tip (half-width `t^0.1 (1 - t)^0.2`). Three fields of
`[foliage]`, 0 by default, shape any blade or card:

| Field | Meaning |
|---|---|
| `twist_deg` | total twist of the blade about its midrib from base to tip: the spiral of a ribbon |
| `undulation` | amplitude of the waves of the margin out of the plane of the blade, as a fraction of its half-width: the ruffled edge of a kelp blade |
| `undulation_waves` | number of waves along the blade (4) |

The two margins wave a quarter turn apart, so the edge ruffles rather than the blade
waving as a whole. A twisted or undulating blade gets the segments the twist, the waves
(four per wave) and the bend need within the tolerance, at most `foliage.segments`; a
blade without them is meshed exactly as before (the snapshot goldens are unchanged).

**Whorled small leaves** are the phyllotaxis of the species: `leaves_per_node = 3` and a
divergence of 60 degrees for a waterweed.

**Pads.** `foliage.shape = "pad"` is a disc one leaf length across (times `width_ratio`)
centred on the organ site, with a notch of 2 x 0.12 rad from its centre to its edge on
the side of the leaf base. It is a fan of 10 to 48 triangles and is never meshed as a
card; the leaf texture cuts the same notch out of its alpha.

**Floats.** `aquatic.blade_float` adds to every leaf template (of a species without
twigs) a closed ellipsoid of 4 rings of 8 sides at the base of the blade, tinted with
`float_color`, sampled at the opaque stalk texel of the atlas. The large float of
`tip_float` is part of the bark: the swollen end of the stipe.

**Smooth stems.** The aquatic presets use the `smooth` bark pattern with no lenticels
(`coverage = 0`), a faint relief and a low roughness (0.3 to 0.5): a wet, bark-less
stipe or petiole.

**Holdfasts** are axes of the bark part, labelled `root`.

### Materials: translucent-looking without subsurface

glTF 2.0 has no subsurface scattering in its core, and Arbrume writes core materials
only. Kelp blades, seagrass and waterweed leaves are thin and let light through; the
presets approach that look with what glTF carries:

- a **lighter, more saturated base colour** than land foliage (golden brown for kelp,
  bright green for waterweed and tape grass), which reads as light passing through;
- a **low roughness** (0.25 to 0.4), the sheen of a wet surface;
- **alpha mode `MASK`** with a cutoff of 0.5 and double-sided leaves, as for all textured
  foliage. `BLEND` was not chosen: thousands of overlapping blades sorted by object
  draw wrongly in real-time engines, and the cut-out already gives the outline.

What importers do with this, checked with Blender 5.3 (glTF importer, background
script) and from the documentation of three.js: Blender makes a Principled BSDF with the
base colour texture times the vertex colour, the roughness factor, and, for `MASK`, a
*Math* node pair (`texture alpha - 0.5`, then `less than`) into *Alpha* with the
material's render method set to *Dithered*; double-sided materials get backface culling
off; *Transmission*, *Subsurface* and *Translucency* stay 0. three.js `GLTFLoader` makes a
`MeshStandardMaterial` with `alphaTest = 0.5`, `side = DoubleSide`, `transparent = false`.
For a translucent render, add it in the renderer: in Blender mix a *Translucent BSDF*
(or set *Subsurface Weight*) into the leaf material, as the underwater renders below do;
in three.js use `MeshPhysicalMaterial` with `transmission` or a custom shader.

### Textures

The leaf atlas is synthesized as for any species ([Textures](textures.md)). Strap leaves
use the outline of their shape, with parallel veins on seagrass and tape grass and none
on kelp; pads draw their notch into the alpha of the atlas and take open venation.

## Current sway: the wind data under water

`--wind` bakes the same per-vertex data under water ([Wind](wind.md)): the normalized
positions `x0..x3` along the branches of the lineage of a vertex, its own branch and its
flutter, and the table of branches with their pivots, tangents, lengths, radii and
deflection shapes. A shader that sways plants in wind sways them in a current with three
changes:

- **slower:** water is about 800 times denser than air, and kelp moves with the swell
  and the current, not with gusts: periods of seconds, not fractions of a second;
- **larger:** flexible stems "go with the flow"; let the amplitude be a fraction of the
  branch length (0.1 to 0.3) rather than a few percent, and keep it in the direction of
  the current, whose mean lean the plant already carries from the force pass;
- **in phase along a frond:** use a phase that grows with `x0` (`phase = k x - w t`), so
  a wave travels from the holdfast to the tip, and a flutter of the blades (`flutter`)
  that lags behind.

```glsl
// After the wind shader of docs/wind.md: a wave along the frond and a slow swell.
float swell = sin(uTime * 0.6 - x0 * 3.0 + shapePhase.w);
offset += uCurrentDir * 0.15 * tangentLength.w * u * (0.7 + 0.3 * swell);
offset += uv3.y * 0.05 * vec3(sin(uTime * 1.3 + p.y * 2.0), 0.0, cos(uTime * 1.1 + p.x * 2.0));
```

Holdfasts and rhizomes are flagged as roots and carry the wind data of the base of
their stem, so they stay still. Keep vertices under the water surface in the shader by
clamping their height, since the baked data does not know the surface.

## Presets

All six have `variation.weight = 0`: the random sampler never draws them, since an
aquatic plant is only itself in water and drawing them would change the random sets of
earlier versions. A scene that provides water draws them by name (the forests below).
Sizes at the stated age, in the water depth of the species, seed 1:

| Preset | Kind | What it looks like | Water | Age | Height | Width | Triangles |
|---|---|---|---|---|---|---|---|
| `kelp_like` | giant kelp, parametric | conical holdfast, a dozen fronds of ruffled blades each with a float, canopy spreading on the surface | 10 m | 14 | 10 m | 16 m | 51 k |
| `bull_kelp_like` | bull kelp, parametric | thin stipe swelling upwards, one large float at the surface, a tuft of long ruffled blades trailing on it | 10 m | 12 | 10 m | 5.6 m | 3.2 k |
| `seagrass_like` | seagrass meadow, parametric, rhizome | a branching rhizome under the sand, a ramet of arching strap leaves at every node | 1.5 m | 10 | 0.8 m | 3.3 m | 11 k |
| `ribbon_grass_like` | tape grass, parametric, runners | rosettes of long twisted ribbons on runners, the longest reaching the surface | 0.9 m | 8 | 0.9 m | 3.7 m | 6.1 k |
| `waterweed_like` | waterweed, self-organizing | slender stems in whorls of three small leaves, growing most near the surface and spreading under it | 1.4 m | 12 | 1.4 m | 0.8 m | 52 k |
| `water_lily_like` | water lily, parametric, rhizome | a thick rhizome bearing three crowns of long petioles, each ending in a notched pad on the surface | 0.9 m | 7 | 0.95 m | 1.8 m | 2.6 k |

Ages: `kelp_like` 8 to 20, `bull_kelp_like` 6 to 16, `seagrass_like` 6 to 14,
`ribbon_grass_like` 5 to 12, `waterweed_like` 8 to 16, `water_lily_like` 4 to 9. Each
jitters lengths, counts and colours in `[variation.jitter]`. No flower is modelled.

```sh
arbrume gen --species kelp_like --age 14 --out kelp
arbrume gen --species kelp_like --age 14 --water-depth 6 --current 0.3,0 --out kelp_current
arbrume sheet --species water_lily_like --ages 5,8 --azimuths 20,110 --out lily.png
```

## Scenes: a kelp forest and a pond

[Forests](forest.md#aquatic-species) place aquatic species under
`placement.water_level`, land species above it. `examples/underwater/` has two
configs with their heightmaps:

- `kelp_forest.toml`: 60 x 60 m of rocky seabed shelving from 5 to 17 m deep
  (`seabed.png`), giant kelp from 8 m down, bull kelp from 5 to 9 m;
- `pond.toml`: a pond 24 x 20 m and 1.6 m deep (`pond.png`), water lilies on the open
  water, tape grass in the shallows, waterweed in the deepest part, rosette herbs on the
  bank.

```sh
arbrume forest --config examples/underwater/kelp_forest.toml --out out/kelp_forest
arbrume forest --config examples/underwater/pond.toml --out out/pond
```

Both were rendered for the look of this page with Blender 5.3 Cycles from the instanced
`.glb` files and the heightmaps: the leaf materials mixed with a *Translucent BSDF*
(see below), the water as a principled volume, lit by a large area light just under the
surface so that the light fades with depth (a sun outside the volume would be absorbed
entirely by a world volume), and a refracting plane for the surface; no caustics. The
render script is not part of the repository.

## Command line, Python and Blender

- `arbrume gen` and `arbrume sheet` take `--water-depth METERS`, `--current X,Z` (m/s)
  and `--water-attenuation PER_METER`, which set or change the water of the
  surroundings ([Command line](cli.md)).
- `arbrume.generate(..., water=8.0)` or `water={"surface": 8.0, "current": [0.3, 0, 0]}`,
  and the `water` key of a `surroundings` dict ([Python](python.md)).
- In Blender the *Water* sub-panel of a plant takes the surface from the species, a
  height above the plant or the top of an object (a plane), and a current along X and
  Y; *Add > Mesh > Arbrume Forest* takes a water height or object ([Blender](blender.md)).

## Cost

Generation of one plant, release build, seed 1, no textures, median of 7 runs of
`arbrume gen --manifest`, on the AMD Ryzen 7 9700F of [Growth](growth.md) while a Blender
test run was using the machine: values to compare, not targets.

| Preset | Age | Nodes | Leaf sites | Triangles | Grow (ms) | Mesh (ms) |
|---|---|---|---|---|---|---|
| `kelp_like` | 14 | 635 | 425 | 51 185 | 0.34 | 1.00 |
| `bull_kelp_like` | 12 | 178 | 28 | 3 242 | 0.11 | 0.24 |
| `seagrass_like` | 10 | 299 | 480 | 10 794 | 0.57 | 0.43 |
| `ribbon_grass_like` | 8 | 167 | 280 | 6 125 | 0.37 | 0.28 |
| `waterweed_like` | 12 | 7 045 | 21 135 | 51 942 | 11.74 | 4.21 |
| `water_lily_like` | 7 | 236 | 21 | 2 555 | 0.28 | 0.20 |

The aquatic passes are linear in the nodes and leaf sites; a meadow grows its ramets one
after the other (a few tenths of a millisecond for 60 ramets of seagrass). The
self-organizing waterweed costs what its light competition costs: 12 ms for 7 000 nodes,
like the self-organizing trees per node. `arbrume forest` on `kelp_forest.toml` (142
trees, 7 variants, textured) and `pond.toml` (123 trees, 12 variants) each finish in well
under a second.

## Limits

- The forces are a first-order static pass: no waves, no oscillation, no iteration to
  equilibrium, and the drag is not recomputed on the bent shape. Fronds that reach the
  surface are held by it; a strong current that would pull a canopy under is not
  modelled.
- Leaves are rigid templates: a ribbon that reaches the surface is tilted until its tip
  is under it instead of bending to lie along it; the random turn the mesher gives a
  leaf may still lift a floating blade a few centimeters above the surface (keep
  `orientation_variation_deg` small on long floating blades).
- Ramets of a meadow are grown alone: no shading between them, no obstacles; a meadow
  is flat (the rhizome lies at the height of the base).
- Stands (`grow_together`) and painted guides grow aquatic species without their
  aquatic stages.
- The large float of `tip_float` is meshed as part of the stipe and has no closed cap
  where it narrows back to the tip.
- No flowers; no epiphytes, no fouling, no decay of old blades.
- No subsurface or transmission in the exported materials (see above).

## References

Read on 2026-11-20.

- FAO, *Kelp resources*, chapter on *Macrocystis* (Fisheries Technical Paper),
  <https://www.fao.org/4/x5819e/x5819e0a.htm>.
- Wikipedia, *Macrocystis*, <https://en.wikipedia.org/wiki/Macrocystis>.
- Wikipedia, *Nereocystis*, <https://en.wikipedia.org/wiki/Nereocystis>.
- Wikipedia, *Zostera marina*, <https://en.wikipedia.org/wiki/Zostera_marina>.
- Wikipedia, *Elodea canadensis*, <https://en.wikipedia.org/wiki/Elodea_canadensis>.
- Wikipedia, *Vallisneria*, <https://en.wikipedia.org/wiki/Vallisneria>.
- Wikipedia, *Nymphaea alba*, <https://en.wikipedia.org/wiki/Nymphaea_alba>.
- Wikipedia, *Photic zone*, <https://en.wikipedia.org/wiki/Photic_zone>.
- Wikipedia, *Drag coefficient*, <https://en.wikipedia.org/wiki/Drag_coefficient>.
- Lee, Z., Shang, S. and Stavn, R. 2018. AOPs are not additive: on the biogeo-optical
  modeling of the diffuse attenuation coefficient. Frontiers in Marine Science 5:8,
  <https://www.frontiersin.org/journals/marine-science/articles/10.3389/fmars.2018.00008/full>.
- Burnett, N. P. and Koehl, M. A. R. 2022. Ecological biomechanics of damage to
  macroalgae. Frontiers in Plant Science 13,
  <https://www.frontiersin.org/journals/plant-science/articles/10.3389/fpls.2022.981904/full>.
- Lin, J. et al. 2016 (Kd of ultraviolet to short visible radiation, ocean gyres to
  turbid coasts), Journal of Geophysical Research: Oceans, doi 10.1002/2015JC011528, as
  summarized by a web search; not read in full.
- Celis-Plá, P. S. M. et al. 2021, Frontiers in Plant Science, doi
  10.3389/fpls.2021.622150, as summarized by a web search; not read in full.
- Reed, D. C. and Foster, M. S. 1984. The effects of canopy shading on algal recruitment
  and growth in a giant kelp forest. Ecology 65(3), as summarized by a web search; not
  read in full.
- Koehl, M. A. R. and Wainwright, S. A. 1977. Mechanical adaptations of a giant kelp.
  Limnology and Oceanography 22(6), as cited by Burnett and Koehl (2022); not read.
- Seagrass clonal growth (rhizome segment per leaf, ramets produced by rhizome
  elongation), as summarized by a web search of
  <https://www.vliz.be/imisdocs/publications/ocrd/67179.pdf> and
  <https://link.springer.com/article/10.1007/s00442-008-1251-3>; not read in full.
