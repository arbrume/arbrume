# Levels of detail

A plant is drawn at many screen sizes. Arbrume gives it a chain of levels: a few meshes
built from the same plant graph, each within a triangle budget, and a billboard for the
distance. This page describes how the levels are made, the options and their defaults,
what was looked at to choose them, and what they cost. [Wind](wind.md) describes the wind
data every level can carry.

```rust
use arbrume_gen::{BillboardOptions, GenParams, Generator, LodSettings, preset};

let species = preset("broadleaf").unwrap();
let mut generator = Generator::new();
let plant = generator.generate(&species, &GenParams::new(7, 20))?;
let settings = LodSettings {
    billboard: Some(BillboardOptions::default()),
    wind: true,
    ..LodSettings::default()
};
for level in generator.lod_chain(&plant, &species, &settings)? {
    println!("{:?}: {} triangles, up to {:.0} px", level.kind, level.triangles, level.max_screen_height);
}
```

From the command line, `arbrume gen --lods 3 --billboard --wind` writes the chain
([Command line](cli.md)).

| Entry point | Use |
|---|---|
| `Mesher::lod_chain(graph, species, base, &LodOptions)` (`arbrume-mesh`) | plans the mesh levels: options, counts, budget and screen size of each; builds nothing |
| `arbrume_mesh::lod::level_options(base, quality, lod, radius_cap)` | the mesh options of one quality |
| `Generator::lod_chain(&plant, species, &LodSettings)` (`arbrume-gen`) | meshes every level and bakes the billboard; returns `LodMesh` values |
| `bake_billboard(mesh, &BillboardOptions)` (`arbrume-lod`) | the billboard part of any mesh |
| `write_glb_levels(&[GlbLevel], options, &ExportOptions, out)` (`arbrume-export`) | all levels in one `.glb` |

## Mesh levels

Detail is removed from the graph, never by decimating a mesh. A level is the mesh of the
graph with the options of one quality `q` in `(0, 1]`, level 0 being the base options
unchanged (`q = 1`):

- **Tolerance** `e = e0 / q^2`: fewer rings and sides on every axis and fewer segments on
  every leaf blade ([Meshing](mesh.md)). Sides, rings and blade segments drop roughly in
  proportion to `q`.
- **Radius cut**: an axis whose base radius is below `radius_factor * e` is dropped with
  everything it bears, provided that nothing it bears reaches farther than
  `reach_factor * e` from its base (`MeshOptions::max_drop_reach`). Its leaves are
  reattached to the nearest point of its nearest kept ancestor, on the surface of that
  axis (`OrphanLeaves::Reattach`), so no leaf floats, and the reach limit bounds how far a
  leaf moves, which keeps the outline of the crown. The cut never reaches half the base
  radius of the thinnest root axis, so a level keeps its trunk.
- **Leaf prefix**: the leaf buffer keeps its first fraction `q` of leaves, each enlarged
  so that the total leaf area is preserved. Leaves are stored in a random order that
  depends only on the seed and the organ index, so the leaves of a level are exactly the
  first leaves of every level above (the stochastic simplification of Cook et al. 2007,
  used for foliage by Livny et al. 2011 and Pirk et al. 2012).
- **Twig templates**: a twig keeps `max(1, round(q^twig_exponent n))` of its `n` leaves,
  spread along its stem and enlarged by `sqrt(n / kept)` (`MeshOptions::twig_leaf_fraction`).

**Budgets.** Level 0 has the triangles of the base options; level `k` has the budget
`triangle_ratio^k` times that. Its quality is searched with the planning pass of the
mesher (`Mesher::counts`), which builds no buffer: starting from the quality the ratio
predicts, the quality is multiplied by the square root of the budget over the count,
times 0.97 so that it lands below, at most six times; the largest count within the budget
and within the count of the level above is kept. Every level is therefore within its
budget and smaller than the one above (the tests check both on presets). The budget of a
plant whose coarsest mesh still exceeds it is missed rather than met by dropping more.

**Screen size.** A level stays faithful while its tolerance projects to at most
`pixel_error` pixels. For a plant of height `h` and a level of tolerance `e`, that is
while the plant spans at most `pixel_error * h / e` pixels of screen height:
`LodLevel::max_screen_height`, written to the metadata of each level as
`max_screen_height`. Switch from level `k - 1` to level `k` below the hint of level `k`.
The hint is geometric: the thinning of the leaves follows the same quality but is not
measured separately.

### `LodOptions`

| Field | Default | Meaning |
|---|---|---|
| `levels` | 3 | mesh levels, level 0 included |
| `triangle_ratio` | 0.3 | budget of a level over the budget of the level above |
| `radius_factor` | 1 | axes thinner than this many tolerances may be dropped |
| `reach_factor` | 10 | ... if what they bear stays within this many tolerances of their base |
| `twig_exponent` | 0.5 | a twig keeps the fraction `q^twig_exponent` of its leaves |
| `pixel_error` | 1 | projected tolerance of the screen-size hint, in pixels |

The defaults were chosen by eye on the contact sheets described below; they are starting
points, not measured optima. A first version without the reach limit dropped whole limbs
at level 2 and gathered their leaves along the trunk, which turned crowns into columns;
the limit keeps the crown outline at the cost of a little more bark.

## Billboards

The last level stands for the whole plant with a few textured quads, baked by
`arbrume-lod` with the CPU rasterizer. Two ways to choose the planes:

- **Billboard clouds** (`BillboardMethod::Cloud`, the default): Décoret et al. (2003)
  choose planes greedily in a discretized plane space, each plane standing for the faces
  within a distance `epsilon` of it. Fuhrmann et al. (2005) adapted them to trees, and we
  take three of their changes: wood (the bark part) and foliage are simplified separately,
  a face counts for a plane by its area times the cosine between their normals, and planes
  near horizontal are penalized (no penalty up to an elevation of the plane normal of 50
  degrees, then linearly up to 60 % for a horizontal plane, their typical values). Plane
  normals come from a Fibonacci lattice of 192 directions over a hemisphere, offsets are
  binned in steps of `epsilon / 2`; the bin refinement, plane tweaking and vertex welding
  of the papers are left out. A plane draws every face valid for it; a face valid for none
  is drawn on the nearest plane of its group, so nothing is lost.
- **Crossed quads** (`BillboardMethod::Cross`): `cross_planes` vertical planes through the
  vertical axis of the plant, each showing the whole plant, and optionally a horizontal
  one: the impostor of most engines.

**Atlas.** Each plane gets a rectangle of one square atlas covering the projection of its
faces, at one texel density for the whole atlas: the largest at which the rectangles,
packed in shelves, fit. Each rectangle is rendered orthographically along the plane
normal, drawing only the faces of the plane. Two PNG textures are embedded:

- base colour: albedo in sRGB and coverage in alpha, the material being alpha-masked at
  `alpha_cutoff` (0.3, the threshold Fuhrmann et al. give for their coverage);
- normal: tangent-space normal (glTF convention), and in alpha the height of the surface
  above its plane, `0.5 + 0.5 h / depth_range` (`Billboard::depth_range`).

Covered texels are spread into their empty neighbours over four texels, so filtering and
mipmaps do not bleed the empty colour into the edges. The quads carry tangents, so readers
need not generate a tangent space. With `bake_occlusion` the sky occlusion of the whole
plant is multiplied into the albedo; it is off by default because renderers then darken
the sunlit side twice (it looked too dark next to the meshes on the contact sheets).
`Billboard::max_screen_height` is the plant height times the texel density: above it the
atlas is magnified and a mesh level should be drawn.

**Which method.** On the contact sheets the clouds keep more of the volume of a crown and
read well from the side and from 35 degrees above; the crossed quads show their seam where
two planes meet and look flat from above. Clouds are the default. Their weak cases: sparse
crowns (an oak-like tree with few, large leaves loses leaves in the planes) and planes seen
nearly edge on from above, which smear. The crossed quads remain an option for very sparse
plants and for engines that want a fixed, small plane count (6 triangles against 24 to 32).
A path tracer needs enough transparency bounces for the overlapping planes of a cloud
(Blender Cycles shows black where its default of 8 is exceeded).

### `BillboardOptions`

| Field | Default | Meaning |
|---|---|---|
| `method` | `Cloud` | `Cloud` or `Cross` |
| `atlas_size` | 1024 | side of the atlas in texels |
| `supersampling` | 3 | samples per texel along each axis when baking |
| `alpha_cutoff` | 0.3 | alpha cutoff of the material |
| `bake_occlusion` | false | multiply the sky occlusion into the albedo |
| `epsilon` | 0.1 | cloud: largest face-to-plane distance, as a fraction of the bounding-sphere radius (6.5 % to 12.5 % in Fuhrmann et al.) |
| `max_planes` | 16 | cloud: most planes |
| `max_wood_planes` | 4 | cloud: most planes for the wood |
| `directions` | 192 | cloud: candidate plane normals |
| `horizontal_penalty` | 0.6 | cloud: penalty of a horizontal plane |
| `penalty_cutoff_deg` | 50 | cloud: elevation of the normal where the penalty starts |
| `max_faces` | 60000 | cloud: faces used to choose planes, a larger mesh being sampled with a stride |
| `cross_planes` | 3 | cross: vertical planes |
| `cross_horizontal` | false | cross: add a horizontal plane |

`LodSettings::billboard_source` (default 1) is the mesh level the billboard is baked
from: level 1 looks the same at billboard distances and costs a third of level 0 to
rasterize.

## In files

With `--lods N`, `--billboard` or `--wind`, `arbrume gen` writes one file per level,
`<name>_lod<k>.glb`, the billboard last; with `--lod-file`, one file whose root node has a
child per level, `<name>_LOD0`, `<name>_LOD1`, ... Each file or level node records
`lod`, `lod_kind` (`mesh` or `billboard`), `lod_budget`, `lod_triangles`,
`max_screen_height` and `quality` in its `extras`. `MSFT_lod` is not written: the
importers we tried do not keep it ([Wind](wind.md#gltf-carriage-trial) has the trial).
With `--instance-leaves` (and `--lod-file`) the leaves of each mesh level are written as
GPU instances of their few templates (`EXT_mesh_gpu_instancing`): the file of an
`excurrent_conifer` of age 15 with three mesh levels and a billboard goes from 7.9 MB to
3.0 MB, a `broadleaf` of age 20 from 9.4 MB to 3.8 MB. The colour of each leaf, its wind
data and the flutter of its vertices are then lost ([Wind](wind.md)).

## Looking at the levels

```
cargo run --release -p arbrume-lod --example lod_sheet -- sheet.png 256 broadleaf:20 pine_like:30
cargo run --release -p arbrume-lod --example lod_sheet -- sheet.png 128@75,35 shrub:8
```

renders one row per plant: the mesh levels, the billboard cloud and the crossed quads,
each framed like level 0 at `SIZE` pixels, from azimuth and elevation `AZ,EL` (20 and 12
degrees by default), and prints the triangles, hints and times of each level.

## Performance

Measured on one 16-thread Windows desktop with the release build, one plant at a time on
one thread (a record of one machine, not a target). Planning is the search of the three
mesh levels; meshing is the sum over the levels, wind data included; the billboards are
baked from level 1 into a 1024-texel atlas with 3x3 supersampling.

| Plant | Level 0 triangles | Levels 1, 2 | Planning | Meshing | Cloud | Cross |
|---|---|---|---|---|---|---|
| `broadleaf` 20 | 58 233 | 16 987, 5 173 | 0.8 ms | 1.8 ms | 230 ms (32 tris) | 167 ms |
| `oak_like` 30 | 46 311 | 13 391, 4 029 | 0.8 ms | 1.1 ms | 166 ms (24 tris) | 148 ms |
| `excurrent_conifer` 25 | 148 707 | 43 546, 12 650 | 2.0 ms | 5.4 ms | 360 ms (32 tris) | 207 ms |
| `pine_like` 30 | 239 690 | 70 772, 21 068 | 3.5 ms | 7.4 ms | 511 ms (32 tris) | 214 ms |
| `open_grown_broadleaf` 25 | 525 373 | 154 661, 45 769 | 5.2 ms | 13.8 ms | 518 ms (32 tris) | 332 ms |
| `birch_like` 25 | 126 102 | 34 091, 10 581 | 1.2 ms | 3.1 ms | 293 ms (26 tris) | 193 ms |
| `shrub` 8 | 73 676 | 21 129, 6 549 | 1.0 ms | 2.0 ms | 304 ms (32 tris) | 220 ms |

The mesh levels cost a few times the meshing of level 0. The billboard dominates: the
atlas is shaded at 9 samples per texel and the source mesh is rasterized once per plane.
A smaller atlas or less supersampling divides the time accordingly; the bake runs on one
thread and a batch runs plants in parallel.

## Limits

- The screen-size hints are geometric estimates and do not account for the enlarged
  leaves; the switch distances should be checked in the target engine.
- Leaves of dropped axes move to the nearest point of the kept axis within eight
  internodes of where their lineage grows from it; a dropped axis with leaves far along
  it moves them by up to `reach_factor` tolerances.
- Billboard clouds miss the bin refinement of Décoret et al., so planes are those of the
  bin centres; sparse crowns and planes seen edge on are their weak cases.
- The atlas is not mipmapped and the billboard does not change with the view direction
  (no octahedral impostor yet).
